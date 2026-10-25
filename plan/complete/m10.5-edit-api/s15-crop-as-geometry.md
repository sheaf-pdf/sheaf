# M10.5 slice 15: Crop as geometry

Status: done · Part of [M10.5: the edit API](README.md)

**Crop as geometry** (S; proposal 27; bugs 16 and 28).
- **`SetPageBoxes`** from the crop dialog: margins against the
  displayed frame, or per-page rects.
- **Trim White** is a resolve job rendering `content_only` at 72 dpi
  (gray 235, MIN_SIDE 18 pt). It commits `Validate`, with
  `read_render` per page.
- **The dialog** previews through `Tx::preview` and the realizer.
- **Deletes:**
  - crop.rs's derived path: `cropped_copy`, `CroppedSource` and the
    `generated::store` calls;
  - its O(N²) id lookup;
  - the pool engine each derived crop source took;
  - `SetPageBase` use for crops;
  - the forced rewrite for crops.
- **Checks:**
  - A crop saves by Append with only page dicts in the section, and
    forms stay clean.
  - Marks, widgets, OCR words and bookmark views keep their stored
    values and land correctly in PDFium and pdf.js (ignored).
  - Of two overlapping trim jobs, the second conflicts and re-plans,
    and both crops hold.
  - A cropped form page keeps its widgets and shows its value, with
    no copy and no cache file.
  - A crop of all 7,000 pages commits in under 200 ms, and
    thumbnails fill at the gate rate.

## Notes

Done. Crop as geometry (proposal 27; bugs 16 and 28): a crop is a
   box edit of the page's `Geom`, one op (`SetPageBoxes`), with no copy.
   An Append of a crop writes the cropped pages' dictionaries alone, an
   OCR flush no longer conflicts with a crop of its page, and a cropped
   page realizes in its source's group overlay. Trim White is a resolve
   stage through the views on a measuring state, so it works on a source
   PDFium refuses and opens no engine of its own; the app runs it as a
   planner that measures again on a Conflict. The Crop Pages dialog
   previews its page through the realizer, on the Header & Footer
   dialog's preview, now shared. A crop of all 7,000 pages commits in
   9.6–9.9 ms, and in 14.6–15.7 ms validated after a delete (asserted
   < 200 ms on every fixture; 445–2,326 ms before M10.5), the strip still
   fills at the gate rate after it, and the corpus counts hold.
   - **The model** (step 1, sheaf-edit, sheaf-doc). sheaf-edit:
 `BoxSpec::{Margins(Margins), Rect(Box2)}` (kebab-case,
 `deny_unknown_fields`); `Margins` moved from header_footer.rs to
 page.rs, its doc general and the header's meaning on
 `HeaderFooterSpec.margins`; `Op::SetPageBoxes { pages, crop, trim,
 bleed, art }` after `ReplacePages` (§6.1), each box a `Patch<BoxSpec>`
 with `serde(default)`, skipped when `Keep`; `fixture::every_op`'s two
 SetPageBoxes ops; `schema/ops-1.0.json` regenerated (it only grew);
 fuzz/op.dict's names. sheaf-doc: `geom::MIN_SIDE` (18 pt);
 `ResolvedOp::SetPageBoxes` ("set-page-boxes"); caps (`Approval`, and
 `JobKind::Trim`, which may submit SetPageBoxes and nothing else);
 tx/pages.rs's `set_boxes` in `lower_op` (progress by page, `Geom(p)`
 a lowering read, written only where it changes; `Keep`, `Clear`
 (crop = media, the others gone), margins off the shown box through
 `from_view` intersected with the visible box, a `Rect` finite and
 within media ± 0.01 pt (`NOISE`) then intersected with it; every
 failure `EditError::Invalid` naming the page's position); describe's
 lines ("Crop pages 3–5", "Remove the crop from …", "Change the page
 boxes of …", a run of crops or of clears one line) and `page_count`;
 prim.rs's heap size; `fx::crop(pages, Margins)` and `fx::crop_to(page,
 Box2)`.
   - **Trim White through the views** (step 2, sheaf-ops crop.rs).
 `trim_white(views, snap, lower, pages, pad, progress) ->
 Result<Trimmed>` and `Trimmed { crops, measured, left }` with
 `Trimmed::lower(&mut Tx)`: a measuring transaction at `snap`
 (`Document::tx_at`) crops each page whose crop differs from its base
 page's own (`base.src.geom(base.index).crop`) back to that box; each
 page of that state is drawn by `views.render` under `content_only()`
 at 72 dpi (its shown width), Rgba, in `Budget::Background`, measured
 with `to_gray` and `content_bounds(.., WHITE)` (235), padded (clamped
 at 0), mapped through the measuring geom's `from_view` and
 intersected with the own box; under `MIN_SIDE` or blank, left; equal
 to the current crop as the op stores it, no op. `lower` records
 `read_render(p, content_only)` for every measured page, then one
 `Rect` crop a page through `apply_batch`. Docs: realize/inputs.rs's
 `content_only()` names Trim White; sheaf-render's lib.rs (only a
 print or an export opens an engine of its own); pool.rs's module
 doc and `Budget::Background`.
   - **The app's crop flows** (step 3). crop_dialog.rs: `CropEvent::
 {Margins { pages, margins }, Trim { pages }}` for `Crop { pages,
 spec }`, `Request::{Margins, Trim}` and `request()`, `TRIM_PAD` (9
 pt; the review moved it to `sheaf_ops::crop::PAD`). page_insert.rs: `crop_pages(opened, ids, margins)`, synchronous
 (`user_tx_at`, one `SetPageBoxes` through `lower_in`, a `Validate`
 commit on `opened.version`, "Crop N pages", the notice "Cropped N
 pages"); `trim_white(opened, ids)` and `trim_from(snap, ids, attempt)`
 on `run_planner("Trim White", ..)`: `trim_white` on views of its own
 over the process's pool as `Job(Trim)`, landed in `tx_at(snap)` with
 `Validate` on `snap.version`, labelled "Crop N pages" for the pages
 it crops; on `EditError::Conflict` below `TRIM_ATTEMPTS` (3) it
 measures again on `model_snapshot()` with the ids still there and
 returns no notice, so the new planner's replaces the conflict's;
 "Trimmed N pages, M left as they were"; `crop_label`,
 `trimmed_notice`. workspace.rs's `toggle_crop` sends each request to
 its flow. jobs.rs's and docview.rs's planner lists name Trim White.
   - **The Crop dialog's preview** (step 4, §8.6). New
 `dialog_preview.rs`: `Preview<K>` (the header dialog's, keyed by any
 `K: PartialEq + Clone`; generation, key, task, picture, failed,
 retired; `begin`, `take`, `drop_retired`, `release`, `element`) and
 `drawn_width`; the header dialog on `Preview<Op>`. jobs.rs's
 `Previewer::page_in(now)`. page_insert.rs's `crop_op(ids, margins)`,
 which the commit and the preview both lower. crop_dialog.rs:
 `CropContext { lower, previewer, .. }` (its `size` gone);
 `draw_preview(now, lower, previewer, Request, scale) -> (PageSize,
 Receiver)`: a `tx_at(now)` with `crop_op` on the previewed page, or
 `trim_white` and `Trimmed::lower` through the previewer's views
 (since the review at priority 0 in the screen's budget,
 `Urgency::Preview`), then the page drawn from `tx.state()` under
 `screen()` in the screen's budget; the dialog's `follow`, `relower`, `release`,
 `asked()` and the hint ("The page below becomes 6.50 × 8.00 in", or
 why it can't be cropped so). workspace.rs: the context from
 `lowering_now()` and `previewer()`, `watch()` passing changes to the
 dialog, textures released on close, on toggle-off and when a
 recovery closes the dialogs.
   - **Crop's derived path, deleted** (step 5). Every user ported:
 crop tests to SetPageBoxes through a shared `common::crop(doc,
 pages, margins)` in sheaf-ops' tests (a Tx at the opened version,
 one op, a `Validate` commit, as the dialog commits); rebase and
 derived-copy tests to `Derive::StripHeaderFooter`, or `Watermark`
 where a copy keeps its header, each keeping what it checked;
 `derived_copy(.., pages: &[u32], ..)`, its callers setting
 `/CropBox` in their `fix` where they need a cropped copy. Docs that
 called a crop a derived copy: derive.rs, `ResolvedOp::SetPageBase`
 and `Derive`, model.rs's `Derived`, store.rs's `MemoryKind::Derived`,
 emit/pages.rs, emit/annots.rs, emit/mod.rs's `Shown`, append.rs's
 `Refused::Held`. gate_7k: "crop all pages" and "validated after a
 delete" time the dialog's path (`cropped`), the strip crops so too,
 and "the edit" row is gone. realize_corpus's local `engine_for` is
 `engine_of`.
   - **The Checks** (step 6). sheaf-doc append_tests'
 `a_crop_appends_only_page_dicts`; sheaf-ops crop.rs's
 `a_crop_appends_only_page_dicts_and_forms_stay_clean` and three
 tests made both ways; forms_pages' cropped form, in a process of its
 own (`common::own_cache`); interop's
 `a_cropped_page_s_records_land_in_pdfjs`; append_corpus's ignored
 `crop_appends_only_page_dicts` (`SHEAF_CROP_ONLY` narrows it).
 sheaf-ops' tests/common gained `two_page_form` (from forms_pages),
 `appended_objects` and `own_cache`; `scratch()` leaves
 `SHEAF_CACHE_DIR` alone in an own-cache child.
   - **Gates, ledgers, docs** (step 7). gate_7k asserts the crop rows on
 every fixture (`CROP_GATE`) and adds Trim White over every page as
 information (`GroupsOpened`, a progress that notes the group engines
 open at each page); retired.rs gains 12 rows and `QUALIFIED_ONLY` 3
 names (`plan`, `Margins`, and `Crop` for describe's `Boxes::Crop`),
 each with its reason, checked with probe files; the
 transitional ledger's 12 → 15 row is deleted and the derived-copy,
 derived-emission and `SetPageBase` rows say crop's part went;
 docs/document-model.md's "Crop as geometry (M10.5 slice 15)", the
 op listing and caps, and its forward references (PageSpace, the
 derived copies, planners and dialogs, emission, the consumers'
 table, the refused source, the overlay, the pool's background, Find,
 blank detection, Text layers); README's Crop pages row.
   - **Deleted,** each confirmed dead by `cargo check --workspace
 --all-targets` and clippy, and the names listed in retired.rs:
 `save::cropped_copy`; sheaf-ops' `crop::{plan, CropSpec (Margins,
 TrimWhite), Margins, CroppedSource, content_margins}` and its
 private `MIN_SIDE` (moved, no row); `engine_for`; `Derive::Crop`,
 its describe arm and every port; `derived_copy`'s crop parameter;
 the dialog's `CropEvent::Crop` and `CropContext.size`; the gate's
 "crop all pages, the edit" row and `Baseline.crop_edit_ms`. Crop's
 derived file, its pool engine and its rayon start went with them.

   Deviations from the plan (the planner's list first, as resolved):
   - **Trim White measures a state, not a new input.** §10.1 has
 `content_only()` draw "over the whole MediaBox"; slice 13 ruled it
 the page's content at its shown box and left the whole box to Trim
 White. A `whole_media` input would have reached the render key, the
 untouched predicate and the emission. Instead Trim White builds a
 measuring transaction at its snapshot that crops each page back to
 its file's own crop box and renders that state under the existing
 `content_only()`: the measurement never depends on sheaf's crop, and
 a page whose only change is its crop is untouched there (its base
 engine). Its decision read stays `read_render(p, content_only)`,
 which is `Page(p)`, as §8.5 says. `ViewInputs`, the key, the
 predicate and the emission are unchanged.
   - **The file's own crop box, not the whole MediaBox** (for Greg): the
 whole media box would show what the file's `/CropBox` hides
 (printer's marks, bleed), which Trim White would take for content
 and crop to about the media box. The own box is the base page's
 (`Source::geom(index).crop`, a fact of the source, not a read), the
 media box for a page with no `/CropBox`. Switching is one line in
 `measuring` (`geom.media` for `own`) and one test
 (`trim_white_stays_within_the_file_s_crop_box`).
   - **`BoxSpec`,** which §6.1 names but never defines, is
 `{ Margins(Margins), Rect(Box2) }`, kebab-case and
 `deny_unknown_fields`: margins off the visible box as the page shows
 after its `/Rotate`, as `crop::plan`'s were; a `Rect` in PageSpace.
 `Patch::Clear` makes the crop the media box and the other boxes
 absent.
   - **Invalid, never clipped:** §3.3's "a crop outside media is ignored"
 is how PDFium reads a file; §6.4 says an op fails and is never
 repaired. A `Rect` off the media box (± 0.01 pt), a non-finite value,
 a negative margin, or a box under `MIN_SIDE` on either side is
 `EditError::Invalid` naming the page; only float noise is clipped.
 `MIN_SIDE` is checked on the box as its caller states it (for
 margins, the kept box in the shown frame; for a `Rect`, its w and h),
 so conversion or noise clipping can't push an exact 18 pt box under.
   - **`MIN_SIDE` moved** from sheaf-ops' crop.rs to `geom::MIN_SIDE`, so
 it has no retired row.
   - **Deletes met before the slice:** "the forced rewrite for crops"
 (bug 13's id-based `removes_content` went in slice 10, and the
 planner's `Why` has no crop arm; the page-dicts-only Append tests are
 its check, since a derived crop's Append did write the copy's
 content); "its O(N²) id lookup" (bug 28: `crop::plan` already used
 O(1) `def()` lookups and `PageSel::Ids` resolves in O(n); it went
 with `plan`, and the 7k crop gate is its check); "the
 `generated::store` calls" (derived copies have been memory blobs
 since slice 3; only combine.rs calls it, which isn't crop's). "The
 pool engine each derived crop source took" went with the copy
 (text.rs's group assertion, and Trim White's 0 group engines at 7k).
   - **The gate's "crop all pages, the edit" row is gone,** with
 `Baseline.crop_edit_ms` (1.65, 1.70 and 1.71 ms, which a dead field
 would fail clippy on): it timed the edit apart from the copy, and no
 copy is left. "Crop all pages" now times what the dialog does (a
 transaction at the opened version, the op, the `Validate` commit),
 and "validated after a delete" the same after the user deleted the
 last page; both compare with slice 2's whole crop (`crop_ms`).
   - **Trim White re-plans at most three times** (`TRIM_ATTEMPTS`):
 nothing in the app re-planned before (`DocView::commit` returns the
 error and sets the failure notice), so the land matches
 `EditError::Conflict`, measures again on `model_snapshot()` with the
 ids still present and returns no notice, the new planner's notice
 replacing the conflict's; past three, or on another error, the
 commit's failure notice stays.
   - **Op JSON stays 1.0,** `schema/ops-1.0.json` regenerated in place
 (`SHEAF_SCHEMA_UPDATE=1`; it only grew, the boxes `Patch` with
 `serde(default)`): nothing is persisted until slice 19's `sheaf
 edit`. tests/ops.rs's envelope test used `set-page-boxes` as an op
 from a newer version; it uses `set-watermark` (slice 16's) now.
   - **Trim White can widen a margins crop:** it measures within the
 file's own box, so a page cropped too tightly by margins is trimmed
 back out to its content (`trim_white_measures_the_page_as_its_file_
 crops_it`), never past the file's box.
   - **An OCR flush during a Trim makes the Trim re-plan:** `read_render`
 records `Page(p)` by value, and `SetOcrText` writes it, so a flush on
 a page after Trim White measured it is a Conflict
 (`a_trim_conflicts_with_a_change_of_what_its_page_shows`), and the
 app measures again. A long OCR run could use up the three tries;
 none did in the tests.
   - **The retired row for `Derive::Crop` is `tx::resolve::Derive::Crop`,**
 not the instruction's `tx::Derive::Crop`: a definition is matched at
 its file's module path (resolve.rs), and a path naming it by its
 last two segments either way. `crop_dialog::CropContext::size` has a
 row too (the dialog takes the page's size from its preview);
 `size` is already qualified-only.
   - **The Trim White gate rows** run on every fixture, the document
 cropped first so each page takes a measuring op. Since the review
 they're alone in `trim_white_on_the_7k` (each fixture in a process
 of its own, `run_parts`' "trim" part), not in `gates()`: a first,
 untimed walk counts the group engines it opened through a progress
 that looks at the pool's open engines at each page (`GroupsOpened`,
 a round trip to the worker a page), reading every page's facts cold
 as the app has by the time it trims; the second, timed with
 `NoProgress`, is the planner's alone and must find the same.
   - Step-level choices, each in its step's report: `Margins` moved to
 sheaf-edit's page.rs (a page-level type now); every box's margins
 come off the page as it showed before the op, so the four fields are
 independent; `set_boxes` records `Geom(p)` as a lowering read for
 every page, not only those it writes (the result depends on the
 page's boxes and turn); a run of crop clears is one line too
 ("Remove the crop from N pages"), a mixed run "Change the page boxes
 of N pages"; group_tests' `flush` helper reads `read_render(p,
 content_only)` as the app does (the same `Page(p)` read); the old
 Derive::Crop stale test went and step 1's SetPageBoxes test took its
 name (the rebase checks stand in `two_pages_shown_as_one_copy_of_a_
 page_conflict` and `a_dialog_on_v_keeps_an_ocr_amend_of_v`, now
 StripHeaderFooter); the dialog's `request()` (the plan's `spec()`)
 returns `crop_dialog::Request`; `TRIM_PAD` lived in crop_dialog.rs
 (the review moved it to `sheaf_ops::crop::PAD`);
 Trim White's label is "Crop N pages" for the pages it actually crops,
 and a trim with no crops still commits its empty transaction, so a
 stale measurement still conflicts; `draw_preview` takes `scale` and
 returns the cropped page's size, since a crop changes the aspect;
 the preview lowers `asked()` (zero margins allowed, so it draws at
 open), `request()` keeping the "Every margin is zero" refusal; the
 preview crops only the previewed page, whatever the Pages chips say,
 as the header dialog's does; it draws `tx.state()` (`Tx::preview()`
 gives lines and effects, not the state); `Previewer::page_in` and
 `page_insert::crop_op` are shared helpers; `close_dialogs_of_another`
 releases both previews' textures (the header's were never freed on
 that path); materialize/tests.rs's derived copies use
 `Derive::Watermark` where the copy keeps its header; untouched_tests
 keeps its derived-copy case beside the new crop case;
 append_rewrite_tests' copy-0-by-a-move pair is renamed "derived
 copy" (the slice 10 notes keep the old names as history);
 forms_pages' copy assertions were replaced in step 5, the copy gone;
 the doc fixes beyond the list (append.rs's `Refused::Held`, store.rs,
 emit/mod.rs's `Shown`, model.rs's `Derived`, `ResolvedOp::
 SetPageBase`); the sheaf-doc Check uses `planned_in_place` (the plan
 must be an Append, then it's written) on append_tests' turned,
 boxed `file(&[2, 2])` with a mark and another tool's comment; the
 forms Append Check is in crop.rs; the cropped-form test saves to its
 file for real; three PDFium tests became both ways; the interop test
 saves the records first, so the crop's Append writes only the page
 over records in the file; the corpus tier prints its counts but
 asserts no baseline count (that needs the full run); the cropped-half
 test's crop moved so the "through the snapshot's geom" fault fails
 (Found); `pad` is clamped at 0; `measured` and `left` overlap for
 blank and too-small pages (measured, then left), a page whose own
 box is under `MIN_SIDE` is in `left` only (never drawn, no read),
 and one whose trim equals its crop in `measured` only (its `Geom(p)`
 read since the review); the
 thread-count test takes its baseline before the first insert (the
 wait was rayon's).

   Found while building:
   - **`sheaf_edit::tape::fmt_amount` panicked on a huge `decimals`**
 (step 1): two new `every_op` fixtures shifted the deterministic fuzz
 test's stream, and `fuzz_bodies_survive_mutated_fixtures` found a
 tape mark with `decimals: 4294967295` (from Op JSON, or a file's
 `/SheafAnnot`): `decimals as i32` wraps to -1 and divides by zero,
 and uncapped it would pad a 4 GB string. `fmt_amount` caps the
 places at the new `tape::MAX_DECIMALS` (15), with a test; the op
 isn't refused, only its display bounded, which protects files with
 such a mark too. No seed: the fuzz test is deterministic.
   - **An op's `Rect` doesn't round-trip every `f32` box** (step 2): a
 `Rect` carries a corner and a size, and the stored crop is
 `Rect::new(x, y, x + w, y + h)`, so where `x0 + (x1 − x0) != x1` in
 `f32` the measuring crop differs from the file's by about an ulp, and
 that page is drawn from its group overlay (the same pixels, a group
 engine's cost) rather than its base engine. About 2% of random
 decimal boxes; nudging w by an ulp fixed 1 of 4,335. Only pages sheaf
 cropped, with inset decimal `/CropBox`es, are affected; integer boxes
 and media boxes from the origin round-trip. An exact fix is a
 corners form of `BoxSpec::Rect`, an op-schema change for Greg.
   - **A fault check passed on a corner** (step 2): with the cropped-half
 test's crop keeping the page's top-left corner, the snapshot's
 `from_view` matched the measuring state's, so mapping through the
 snapshot's cropped geom passed; the crop now hides the top and the
 square's left half, and that fault fails.
   - **A fault slipped past the corpus tier on arrays** (step 6): the
 emitter writing a page's `/Contents` anew on a box change, done for
 a single reference only, passed on 3 sampled files whose `/Contents`
 is an array; widened to arrays, 15 of 29 sampled files fail ("not
 pages").
   - **One `SHEAF_CACHE_DIR` a test process** (step 6): every test in a
 binary sets the same shared value, and opening a file writes to the
 cache (file.rs converts other tools' `/Stamp`s through
 `import::convert_page`, which calls `generated::store`), so "no cache
 file" can only be seen in a process of its own:
 `common::own_cache(test, body)` re-runs the binary for that test with
 an empty cache directory and asserts it's still empty.
   - **`Crop` is a live name** (step 7): the retired rows for
 `Derive::Crop` and `CropEvent::Crop` hit describe's `Boxes::Crop`
 pattern (step 1's) by its last segment, so `Crop` is qualified-only,
 with its reason; both rows still hit their definitions and paths.

   The review of the slice's changes (12 findings, each checked against
   the code; fixed unless said):
   - **A derived flow uncropped its pages** (blocker): `set_base`, which
 Watermark / Background and Remove Header/Footer still use until
 slice 17, replaced a page's `Geom` with its copy's file geometry,
 keeping only the turn, and the watermark was placed on the copy's
 uncropped page. Before this slice the crop was baked into the copy's
 `/CropBox`; now it lives in `Geom` alone, so a watermark on any page
 dropped every rebased page's crop (and trim, bleed and art boxes).
 `set_base` now keeps the page's `Geom` whole (a copy is made of the
 page's base, with its media box and user space) and no longer writes
 it, and reads `Geom(p)`, so a flow planned before a crop made since
 conflicts (as its write over `Geom` did before); `derive::watermark`
 places the mark on each page's state `Geom`, its crop. Test:
 `a_derived_flow_keeps_the_crop` (above). Three sheaf-doc tests set a
 `/CropBox` in a derived copy and expected the page to show it; no
 derived flow changes a copy's boxes now, so untouched_tests' and
 materialize/tests.rs' `copied` make plain copies, and
 `a_derived_page_is_written_as_its_original_object_with_derived_
 content` and overlay_tests' `a_derived_copy_s_page_is_emitted_as_
 the_copy_has_it` keep the copy's `/CropBox` and assert the written
 page has the page's own boxes (no `/CropBox`), not the copy's.
   - **A page Trim White left as already trimmed recorded no read of its
 boxes** (two findings, one fix): its "no op" was decided by its
 crop, but only `read_render` (`Page(p)`) was read, so a crop change
 since landed under Validate unseen, where a page it crops conflicts.
 `Trimmed::lower` now reads `Geom(p)` for every measured page not in
 `left` (`set_boxes` reads it again for those it crops; reads are a
 set). The §8.5 table's Trim White row says so. Test: `a_trim_that_
 leaves_a_trimmed_page_conflicts_with_a_change_of_its_crop`; with the
 read dropped it lands.
   - **The Crop dialog's Trim preview measured at background priority**
 (§10.3 "0 for previews"): `trim_white` takes an `Urgency`
 (`Background`: `u8::MAX`, the background's budget, the planner's and
 the gate's; `Preview`: 0, the screen's budget, `draw_preview`'s), as
 §10.3 has it; the docs' Trim White and app sections say so.
   - **The notes said §6.1 never names `BoxSpec`;** it names it in the op
 listing, never defining it (Deviations, reworded).
   - **The corpus Trim White test couldn't fail on a blank group render:**
 it now asserts every page measured, none left, and a crop for each
 (5 of 5).
   - **The Trim White walk ran in `gates()`,** about 3 minutes of
 information among the asserted gates, its time including a worker
 round trip a page: now `trim_white_on_the_7k` (above, Measured).
   - **`crop_only` copied realize_corpus' `only`:** both call
 `common::narrowed(var, items, path)` (realize_corpus now has `mod
 common`).
   - **Trim White's pad was in three places:** `sheaf_ops::crop::PAD`,
 used by the app's planner and preview, gate_7k and tests/crop.rs.
   - **tests/crop.rs kept a local `open` and `near`:** `common::open` and
 `close` instead.
   - **gate_7k's module doc wasn't rewrapped:** rewrapped, and it names
 `trim_white_on_the_7k`.
   - **No thread-count check covered Trim White:** the thread-count test
 trims the last page each round (above).
   - The review also reran materialize_corpus (the baseline exactly) and
 found no other fast path that writes differently from its slow path.

   Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
   - **The gates** (`gate_7k`, release, every test once at a load of 1.6
 to 2.7, nothing else of this slice's running, 432 s; the Rewrite in
 five more processes of its own):

 | Row | Synthetic | Heavy | Scanned |
 |---|---|---|---|
 | crop all pages, one op on the opened version, committed by value (asserted < 200 ms; slice 14 unasserted: 154, 705 and 1,170 ms with the copy) | 9.81 ms (slice 2: 445 ms) | 9.90 ms (2,326 ms) | 9.61 ms (2,256 ms) |
 | crop all pages but the last, validated after it was deleted (asserted < 200 ms; slice 14: 31–32 ms, the edit on a precomputed copy) | 15.7 ms | 15.3 ms | 14.6 ms |
 | thumbnail strip after a crop and the header (asserted ≥ 310 pages/s, ≤ 96 MB) | 446 pages/s, 80.6 MB weighed, 30.8 MB added (slice 14: 411) | 372 pages/s, 80.7 MB, 71.0 MB (345) | 167 pages/s, 80.7 MB, 81.4 MB (164) |
 | Trim White over every page, cropped first (new; no gate): measured, committed (after the review, alone in `trim_white_on_the_7k`, a warm walk at a load of 1.6) | 38.5 s, 71.3 ms (in `gates()`: 45.9 s, 70.2 ms) | 40.6 s, 79.5 ms (39.5 s, 88.4 ms) | 104.8 s, 74.0 ms (87.9 s, 75.9 ms) |
 | Trim White: pages cropped, left, engines opened, group engines opened | 7,000, 0, 6, 0 | 7,000, 0, 6, 0 | 7,000, 0, 6, 0 |
 | realize + open + render 1,024 px, an edited page (asserted < 15 and < 25 ms) | 12.2 ms (2.3× the warm 5.28 ms) | 21.8 ms (4.4× the warm 5.00 ms) | 33.7 ms (1.5×) |
 | engines after 220 groups: weighed, RSS added, groups open (asserted ≤ 96 MB, ≤ 96 MB, ≤ 6) | 28.8 MB, 37.5 MB, 6 | 28.8 MB, 62.1 MB, 6 | 28.8 MB, 51.1 MB, 6 |
 | one-mark Append on the scanned (asserted < 50 ms) | | | 21.1 ms |
 | snapshot, `Tx::check` of a delete of every page (asserted < 50 µs, < 1 ms) | 0.240 µs, 0.396 ms | 0.271 µs, 0.399 ms | 0.240 µs, 0.393 ms |
 | Find over every page, as opened and headed (no gate) | 11.4 s, 12.0 s | 7.9 s, 9.1 s | n/a |

 The strip fills faster than slice 14's on all three, its groups now
 realized over the source's own bytes rather than a derived copy's,
 each overlay writing the pages' new boxes. Trim White reads every
 page from its source's base engine (the measuring state
 leaves a page whose only change is its crop untouched), which opens
 six times as the background's base engine grows past what the
 screen's leave, as Find Blank Pages' does: 182, 172 and 67 pages a
 second, a page at a time (a cold first walk took 51.9, 65.7 and
 108.2 s). The rest as slice 14 left them: the
 geometry walk 47.8 and 76.6 ms (< 100 and < 250 ms); ready on the
 heavy 361 ms (< 1 s); 200 OCR versions of 1,000 pages at most 256 MB
 (12 versions, 235 MB of steps); AddMark 0.019 ms and a full reorder
 0.980 ms at 20,000 marks; 1,000 mark adds on the heavy 0.033 ms
 (blank 0.021 ms); ready on 20,000 sheaf marks 540 ms (< 1 s); Find
 Blank Pages over the synthetic 11.8 s and 6 engines; Find past six
 pinned groups 11.9 and 8.8 s, 49 engines.
   - **The Rewrite against slice 8's writer** (seven processes of the
 writer, `rewrites_on_the_synthetic_and_heavy_7k` five more times
 beside the harness's two, against slice 8's 134 and 707 ms):
 synthetic 146–152 ms, median 150 ms, 1.12× (1.09–1.13×), RSS added
 49.9–50.9 MB (0.81–0.82×); heavy 714–831 ms, median 764 ms, 1.08×
 (1.01–1.18×), RSS added 251–254 MB (1.00–1.01×). None past the 1.2×
 flag; the slice changed nothing on the writer's path.
   - **The corpus** (../pdf-corpus), every count as slice 14 left them:
 materialize_corpus's identity 4,389 of 4,392 written, 2,269,722
 primary objects surviving, 919,479 copied as they were, 436 changed
 by `keep_only`, 0 failed; 2,901 forms without their first page, 0
 failed; every page duplicated, 4,389, 0 failed; inserted twice into
 the 1040, 1,098, 0 failed; four times, 0 differ; the 2932995 logo, 0
 of 2,037,600 bytes differ. append_corpus: 4,389 marked, 3,770
 appended, 259 rewritten (211 RemovalCheck, 34 BaseObjectsMissing, 14
 BaseUnsound), 360 asked about signatures, 0 failed, 0 derived vetoes
 and 211 on the files' own objects; the oracle finds text gone from
 2,899 of 2,901 page-delete Rewrites (none from 0016064 and 0112654).
 encrypted_corpus: the Rewrite keeps 573 of 573 and the Append 572
 (0630899 declined). The ignored tiers (deleted means deleted,
 realize_corpus's two, the new crop tier) wait for the pre-merge run
 (Greg, 2026-10-20). realize_corpus on five files by
 `SHEAF_REALIZE_ONLY` (4422502, 4427660, 0106273, 0171994, 0630899;
 44 s, debug): the predicate disagrees on 0 of 225 pages and inputs,
 and 4 of 23 pages differ from the Rewrite, all within the
 anti-aliasing allowance (0106273 page 0 by 1 px, 0171994 pages 0, 12
 and 23 by 1, 3 and 4; slice 14 saw 2, which pages varying with what
 the process drew before), 4422502 refused by PDFium and realized
 standalone. The crop tier by `SHEAF_CROP_ONLY` on nine files (those
 five, the 1040, fl100, and the signed 0000772 and 0023352): 9
 cropped, 2 appended with page dictionaries alone, 4 rewritten (the
 1040 and fl100 for RemovalCheck, their usage rights; 0630899 and
 4422502 BaseUnsound), 3 asked about their signatures, 0 too small, 0
 failed; step 6's sample of 29 files had none fail either. After the
 review's fixes materialize_corpus, append_corpus and
 encrypted_corpus give every count above again, the narrowed
 predicate tier (4422502 and the 1040) and crop tier (the 1040) run
 through `common::narrowed`, and the interop tier passes (8 ignored
 tests).
   - **Suites:** `cargo test --workspace` 1,376 passed and 37 ignored
 (slice 14: 1,352 and 34; after step 5, 1,371 and 34; after the
 review's fixes 1,379 and 38: the derived-flow pair, the re-trim
 conflict, and `trim_white_on_the_7k`): sheaf-doc +10
 (boxes_tests' 7, the page-dicts Append, the flush after a crop,
 `trim_white_sets_page_boxes_alone`; two renamed), sheaf-edit +1,
 sheaf-ops +11 (crop.rs's nine new and three made both ways, less
 src/crop.rs's unit test, ported to sheaf-doc), sheaf-app +2 (the
 overlapping trims, the dialog's preview); ignored +3 (the crop tier,
 the pdf.js pair). fmt and clippy (`-D warnings`, all targets) are
 clean, and so is clippy with sheaf-app's, the pool's and gate_7k's
 Linux-only gates switched to `cfg(any())`, as macOS and Windows
 build them; `cargo check` and clippy (`-D warnings`) on
 `x86_64-pc-windows-msvc` pass for sheaf-doc, sheaf-edit and
 sheaf-util with their tests and for sheaf-render, sheaf-print,
 sheaf-ops and sheaf-ocr with all targets; sheaf-plugin, sheaf-app and
 sheaf-cli can't be checked here (no `lib.exe` for their C build
 scripts), as before. The release properties: Append ≡ Rewrite at
 1,024 cases and the replay property at 4,096 pass with the new
 "set-page-boxes" op (59 s together). The interop tier (qpdf; pdf.js
 6.4.299 on node 24.22): 9 passed (slice 14: 7), `qpdf --check` clean
 on every output, pdf.js reading the cropped page both ways as `view
 72 20 582 742`, `annot Square 323 323 377 377`, `annot Widget 100 710
 300 740`, `bookmark XYZ 300 400`, "sheaf" at (300, 376) 100 × 24 and
 "interop" at (150, 208) 140 × 24 (the stored boxes exactly).
   - **The reviews:** the plan was reviewed adversarially before the
 steps started, and its findings are the deviations above, as
 resolved. Each step checked its new checks by switching their
 behavior off (below).

   Checked by tests:
   - **The op** (sheaf-doc tx/boxes_tests.rs): `margins_crop_the_shown_
 sides` (ported from sheaf-ops' crop.rs, the same rects), margins off
 a crop, a `Rect` within media, float noise clipped, partly off the
 page Invalid, `Clear` back to media, every Invalid case and its
 message with the 18 pt boundary, trim, bleed and art set and cleared,
 only `Key::Geom` prims (none, and no version, when nothing changes),
 and the lines; tx/tests.rs's resolved-op list (24 kinds), the caps
 table with `Job(Trim)`, `trim_white_sets_page_boxes_alone`
 (RotatePages refused, no ids drawn); sheaf-edit's
 `page_boxes_are_kept_cleared_or_set`, the JSON forms and misspelled
 fields; stale_tests' `a_crop_lands_beside_another_page_s_delete_and_
 conflicts_with_its_own_page_s_crop` (a Conflict on `Geom(p0)`, "“Crop
 page 1” changed the same data"); group_tests'
 `an_ocr_flush_lands_after_a_crop_of_its_page` (the ledger row's
 check); the history property's "set-page-boxes" (random margins,
 rects and clears, some invalid) in the replay and Append ≡ Rewrite
 properties; fuzz.rs's `applying()`.
   - **Trim White** (sheaf-ops tests/crop.rs): `trim_white_finds_the_
 content` (ported), `trim_white_measures_the_page_as_its_file_crops_
 it` (the whole square found after a crop hides half of it, the pool
 opening only the base engine), `trim_white_stays_within_the_file_s_
 crop_box`, `trim_white_crops_a_turned_page_in_page_space` (`/Rotate
 90` off the origin, with and without a sheaf turn, PDFium's render of
 the saved file at least 95% ink), `trim_white_leaves_what_it_can_t_
 crop_and_commits_nothing_more`, `trim_white_works_on_a_source_pdfium_
 refuses` (`revision_cut_out`), `trim_white_on_the_corpus_form_pdfium_
 refuses` (4422502, 5 of 5 pages trimmed, within their file's boxes),
 `of_two_overlapping_trims_the_second_conflicts_and_re_plans` (B a
 Conflict on `Geom` naming A, measured again on A's version, all four
 pages holding their squares), `a_trim_conflicts_with_a_change_of_
 what_its_page_shows` (`Page(p)`, "Recognize text") and, from the
 review, `a_trim_that_leaves_a_trimmed_page_conflicts_with_a_change_
 of_its_crop` (`Geom(p)`, "Uncrop"); `a_derived_flow_keeps_the_crop`
 (both ways, from the review: two cropped pages, one watermarked and
 one's sheaf header stripped, keep their `Geom` and save that
 `/CropBox`, the mark centered on the crop, and a watermark planned
 before a later crop a Conflict on `Geom`).
   - **The app** (headless, Linux): `two_overlapping_trims_both_land`
 (the second re-plans, "Trimmed 1 page", each page's crop what
 `trim_white` finds, undo "Crop 1 page" then "Crop 3 pages");
 `the_thread_count_is_constant_across_20_inserts_and_crops` (crops
 through the synchronous op and, since the review, a Trim White of the
 last page each round through its planner and the pool; its baseline
 before the first insert);
 `find_and_patterns_search_through_the_pool` (a SetPageBoxes crop);
 crop_dialog's `the_crop_dialog_previews_the_crop_and_drops_a_stale_
 result` (margins and trim previews pixel-equal to the page as
 applying draws it, the size and hint, too-large margins' reason, a
 gone page previewing the first, the same request not lowered again,
 a stale generation dropped, a failure keeping the last picture).
   - **The Checks:** `a_crop_appends_only_page_dicts` (sheaf-doc: the
 written set exactly the cropped pages' own objects, every key
 `ObjKey::Page` and every object a `/Type /Page` dict, `/Contents`,
 `/Annots` and `/Parent` the base's references, the raw `/CropBox` the
 crop, reopened alike); `a_crop_appends_only_page_dicts_and_forms_stay_
 clean` (a form's section page 1's dictionary alone, `forms::check`
 clean, values and `/P`s kept); `margins_crop_and_marks_follow`,
 `a_bookmark_on_a_cropped_page_lands_on_the_same_content` and
 `a_crop_leaves_ocr_text_on_its_words`, each both ways, and the
 highlight pair, each asserting the stored PageSpace values unchanged
 by the crop; `pages_alike_cropped_apart_stay_apart` (no copies, each
 link to its page both ways); text.rs's `chars_come_from_each_page_s_
 realization` (a crop's content view `EngineKey::Group` of the
 primary, its base the primary: slice 14's leftover); untouched_tests'
 crop case (not untouched under any consumer's inputs);
 `a_cropped_form_page_keeps_its_fields(_appended)` in a process of its
 own (the screen's render through views shows the value, the base the
 primary, no derived blob in the store, 4 fields and 4 widgets saved
 with their values, `/CropBox` and drawn value, `/Annots` as they
 were, the cache directory empty); interop's `a_cropped_page_s_
 records_land_in_pdfjs(_appended)` (ignored; below); append_corpus's
 `crop_appends_only_page_dicts` (ignored; below).
   - **Fault checks** (each step's, restored from its own copy and checked
 with `cmp`; no proptest seed added): step 2's 5 (no measuring ops; the
 whole media box for the own box; `read_render` skipped; rendering
 with `engine_for` by base index; mapping through the snapshot's
 cropped geom, once the test's corner moved); step 3's (landing
 without the re-plan: "Crop 3 pages failed: “Crop 3 pages” changed the
 same data"); step 5's (the crop comparison dropped from the untouched
 predicate: text.rs's group assertion and untouched_tests' crop case
 fail); step 6's 4 (the emitter writing `/Contents` anew on a box
 change, a single reference and then arrays: the sheaf-doc test, the
 forms Append test and 15 of 29 corpus files; a crop through a
 derived copy: the plan a Rewrite, and the forms, no-copy and alike
 tests; a cache file written in `own_cache`'s body; the emitter
 writing the media box as the crop: both pdf.js tests): all caught.
 Step 7's: the crop's lowering writing `Page(p)` too, a new value,
 as the old rebase did (the ledger row's check fails at the flush's
 commit); retired.rs's 12 rows each hit by a probe file defining it at
 its path and naming it, and the three qualified-only names hit when
 qualified and not when bare (probe files in a package cargo doesn't
 build, deleted). The review's fixes: `set_base` writing the copy's
 geometry with the page's turn again (the derived-flow pair's `Geom`
 assertion fails), its `Geom(p)` read dropped (the late watermark
 lands), `derive::watermark` placing the mark on the copy's geometry
 (the mark 21 and 15 pt off the crop's middle), and `Trimmed::lower`'s
 `Geom(p)` read dropped (the re-trim lands): all caught.

   For Greg:
   - **Trim White measures within the file's own crop box,** not the
 whole MediaBox §10.1 and the slice 13 and 14 notes said: the whole
 box would take what the file's `/CropBox` hides (printer's marks,
 bleed) for content and crop to about the media box. Switching is one
 line in `crop::measuring` and one test (Deviations).
   - **Op JSON stays 1.0** with `ops-1.0.json` regenerated in place (it
 only grew; nothing is persisted until slice 19). A minor bump would
 change the envelope and schema tests.
   - **A `Rect` box is a corner and a size,** so about 2% of decimal
 `/CropBox`es don't round-trip exactly in `f32` (Found); such a page
 measures from its group, with the same pixels. A corners form of
 `BoxSpec::Rect` would make it exact, at the cost of a schema change.
   - **Trim White re-plans at most three times;** an OCR flush on a page
 it measured makes it re-plan, so a long OCR run beside a Trim could
 use them up (the notice then says it changed the same data).
   - **Trim White at 7,000 pages takes 38–41 s** on the synthetic and
 heavy and 105 s on the scanned (no gate), a page at a time in the
 background's budget; a later perf pass could walk it
 (`EnginePool::walk`) or render smaller.
   - **The crop's notices:** "Cropped N pages"; Trim White's "Trimmed N
 pages, M left as they were", and its undo line "Crop N pages" for the
 pages it cropped. Wording yours.
   - **The fmt_amount fix** (Found) is outside the plan: a mark's display
 with a huge `decimals` is capped at 15 places rather than panicking.

   Left for later slices:
   - **Slice 16:** the watermark generator and its dialog's preview, on
 the now-shared `dialog_preview::Preview` (`drawn_width`, `element`,
 `drop_retired`, `release`) and `Previewer::page_in`; slice 16 can
 check "no cache file" with `common::own_cache`.
   - **Slice 17:** derived copies' remaining users (header and footer,
 watermark), `derived_copy`, the rest of `Derive` and `Tx::
 set_page_base`, with derive.rs; the header preview's
 `strip_stamps_of`.
   - **A later perf pass:** Trim White over 7,000 pages (above), beside
 Find's and the realize gate's.
   - **Follow-ups:** the `Closure::stale` probe (slice 11's Unprobed),
 untouched: no step changed that walk.
