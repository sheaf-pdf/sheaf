# M10.5 slice 18: OCR as content

Status: done · Part of [M10.5: the edit API](README.md)

**OCR as content** (M; proposal 26; bug 14).
- **The ops:** `SetOcrText` and `ClearOcrText`, in an Ocr group.
- **Rendering:** OCR renders `content_only`, records the `PageDef`
  it rendered (`read_render`), and converts pixels with the
  render-time `Geom`.
- **Flushes:** `Validate` per flush. A changed page's result is
  dropped and re-queued.
- **Removing saved layers:** by the `/SheafBlock` stream marker, or
  by the `/ShOCR` marker inside merged streams. A legacy
  `/SheafOcrLayer` is removed together with its leading `q`.
- **Re-OCR** replaces a saved layer.
- **Deletes:**
  - `text_layer::install`, `strip` and `has_layer`, and with them
    slice 9's legacy-shape OCR emission;
  - the presence-means-unsaved remnants;
  - the plugin API's OCR overrides;
  - `/SheafOcr` as the OCR font's resource name, and
    `/SheafOcrLayer` as a written marker (new layers use `/Sh…`
    names and `/SheafBlock`).
- **Checks:**
  - **Finding and saving:**
    - OCR, then Find, hits words before saving;
    - OCR plus an incremental save appends one stream per page, with
      the glyphless font once per file;
    - an empty layer writes nothing.
  - **Removing saved layers:**
    - replacing a saved layer gives a Rewrite with the old words
      absent;
    - `ClearOcrText` removes a saved layer, including one inside
      streams another tool merged;
    - removing a legacy layer leaves q/Q balanced.
  - **During a run:**
    - a rotate or crop during a run doesn't conflict, and the words
      land correctly;
    - a `ReplacePages` drops that page's result, which is re-queued;
    - OCR of a page with an unsaved header doesn't record the
      header's text.
  - **Undo:** an OCR run is one undo step, and undo during it stops
    the flushes.

## Notes

Done. OCR as content (proposal 26; bug 14): a page's recognized text
   is a §11.4 block, a `Layer` with a `BlockId` on its stack, drawn as a
   stream of its own, `/SheafBlock << /K /Ocr /B (b…) >>`, after the
   page's isolated content (`QClose`) and before the generators drawn
   over it, its text `/ShOCR /Sh… BDC q BT 3 Tr … ET Q EMC` with the list
   `<< /SheafOcrBlock (b…) >>` and the one glyphless font a file under
   fresh `/Sh…` names. The legacy installer went (`text_layer::install`,
   `strip`, `has_layer`, `to_content`, `/SheafOcrLayer` and `/SheafOcr`
   as written names) with the emitter's special cases for it
   (`replaced`, `fresh_balance`, `page_res`' inline resources), and the
   reading order is §11.4's again: the page's own text, the recognized
   words, then the header. Saved layers are removable: `SetOcrText` and
   `ClearOcrText` read the page's blocks as they lower and add
   `DropStreams(StreamSel::SheafOcr)` (a block that's a stream, or an
   older sheaf's `q` and layer, both or neither) and
   `DropMarked(MarkedSel::SheafOcr)` (a block another tool merged into a
   stream), so a re-OCR or a Clear over a saved layer plans a Rewrite
   (`REMOVES | REPLACES_TEXT`, `RemovedKind::OcrLayers`) with the old
   words gone; a page whose content can't be read fails the op with the
   new `EditError::Content { op, page, why }`. In a reopened file a saved
   block is found by its marker (`Marker::Ocr`) and kept in the
   static-over slot, so nesting never grows, a file saved with OCR alone
   included. An OCR run reads a page that changed during it again, in
   rounds in the run's group (twice at most), and counts a page it can't
   read as failed. The plugin API's 0.3 lost `ocr.page-text-layer`; 0.2's
   answers none until slice 20. Also: an ignored OCR corpus tier
   (`SHEAF_OCR_ONLY`), pdf.js and qpdf checks of the blocks, the 7k
   harness's OCR rows with an asserted census, retired.rs, the ledgers
   and the docs. The corpus counts hold, and every asserted gate holds.
   - **The model and lowering** (step 1, sheaf-doc).
 `BlockId::draw(rng, taken)` (random in [1, 2^53)) and `Display`
 (`b{:x}`), round-tripped by `BlockId::parse`. records.rs:
 `Layer { id: BlockId, body: Static }`;
 `ContentStack::with_ocr(Option<Layer>)` in place of
 `with_ocr_text`, keeping the shared empty stack; `ocr_layer()`,
 `with_filters` (deduplicated; `with_filter` calls it),
 `Filter::drops_ocr()`; `StreamSel::SheafOcr` and
 `MarkedSel::SheafOcr`, each documented. tx/mod.rs: `Drawn.blocks`,
 replayed reversed on Relower; `Lower::block_id()` (a pinned id
 first, else a random one; no global uniqueness scan, its doc says
 why); `Lower::content(err, page)`. tx/error.rs:
 `EditError::Content { op, page, why }`, displayed `{op}: {why}`,
 `page` the document position from 0 and `why`
 `ContentError::on_page(pos)`'s text. tx/content.rs: `set_ocr` and
 `clear_ocr` over one `put_ocr` (an empty layer means none; a layer
 replacing the stack's keeps its id, else draws one; nothing put when
 the page is as it was; a lowering read of `Page(p)`, as
 `remove_generated` records); `saved_ocr` reading `src.blocks(index)`
 (an Err is `EditError::Content`) for the filters, `DropMarked` only
 once `cut_together` shows a splice takes the merged ranges together
 (a refusal is `ContentError::Unsafe`). ClearOcrText reports progress
 a page at a time. effects.rs: an added `SheafOcr` filter is
 `REMOVES | REPLACES_TEXT` with `RemovedKind::OcrLayers`, not
 `Filtered`, counted once a page with a removed stack layer; layer
 comparisons include the id. realize/key.rs feeds the layer id and
 tags 4 (`DropStreams(SheafOcr)`) and 5 (`MarkedSel::SheafOcr`).
 text_layer/fixture.rs (`#[cfg(any(test, feature = "fixtures"))]`,
 slice 17's `generate::fixture` shape): `legacy_page()` (an
 unbalanced base, `q 1 0 0 1 20 20 cm` and text in `/F1`, then the
 legacy pair exactly as the installer wrote it, `LEGACY_LAYER` frozen
 and checked against `install` before step 2 deleted it),
 `block_page()`, `merged_page()`, `layer()` and their constants.
   - **Drawn as a block** (step 2). `text_layer::block(layer, id,
 reserved)` writes the block's operators; `OCR_BLOCK`, `OCR_FONT`
 (`ShOANHIY`) and `OCR_LIST` (`Sh6OZT5N`) are its output, frozen.
 `ContentWriter::finish(reserved)` and `named(reserved)` lost
 `given`, and every caller with them (generate.rs ×2, annot.rs ×2,
 the comment summary, writer_tests). emit/content.rs: `page()`
 works out the drawn text (`ocr_over` and a layer with words);
 `stack()` no longer returns early for text, skips `intact()` when
 text is drawn, raises the version to `CID_FONTS` after drawing text
 and `ARTIFACTS` for generators; an unreadable page with text or
 generators is drawn as it is and noted (`cx.unread`: "No recognized
 text, watermark, background, header or footer is drawn on …");
 `wrapped()` draws the OCR block after `QClose`, before the
 generators over the page; `rebuilt()` acts on the filters
 (`DropStreams(SheafOcr)` leaves the stream-level blocks and the
 `legacy_ocr` pair out, a legacy layer stream without its `q` stays,
 counted; `DropMarked(SheafOcr)` cuts `SheafBlock::Ocr` ranges at
 `BlockAt::Marked` through `splice_each`, a refused cut counted);
 isolation is written when generators or text are drawn and the
 balance counted again by `content::kept`; `block(cx, marker, ops)`
 generalized and `recognized()` writing the block, keyed
 `ObjKey::Block`, its resources in the page's `PageRes`.
 emit/mod.rs: `Cx.text_stayed` and its Note; `Cx::sym_dict` inlined
 into `Sink::dict`. `content::LEGACY_OCR` (`b"SheafOcrLayer"`) shared
 by the classifier and the emitter. Slice 17's reading-order hunks
 reversed in sheaf-ops' text.rs (identical to `bf7baf2~1`) and the
 app's find test ("page 5\nTotal 42\nDraft"). The OCR tests moved
 from rewrite_tests.rs and materialize/tests.rs to the new
 materialize/ocr_tests.rs and were ported to blocks.
   - **Kept in their slot** (step 3). `Marker::Ocr(BlockId)` from
 `StreamMark::Block(SheafBlock::Ocr)` (an older sheaf's layer and a
 Flat block stay none); removal.rs' walk falls through for it, so a
 saved block is judged on its decoded words as one of the file's
 objects. `intact()`'s layout is `[blocks_under, Isolate, inner,
 Isolate, Ocr blocks*, blocks_over]`, the trailing OCR blocks peeled
 off by marker before the isolation pair is checked; with no
 generator on the page it's `!backed && (!isolates || unbacked ||
 text)`. `rebuilt()`'s `saved_text`: the stream-level OCR blocks no
 filter drops, in file order, left out of base′ and its balance
 (`out_of`) and referenced after `QClose`, before the model's block
 and the generators over the page, their resource names kept by one
 `content::names_used` pass over those streams; isolation written
 when only saved text is kept, the file's isolating streams reused
 through `theirs`; no pagination cut looked for inside an OCR
 stream. `stack()`'s unmarked case `(true, None)` reads
 `src.blocks(index)` (the memoized read `wrapped()` made) and sends a
 page whose blocks list sheaf's isolation or a stream-level block to
 `rebuilt(&[])`; `wrapped()` takes the `Balance` and no longer fails.
 `stack`, `intact`, `untouched.rs` and `key.rs` still read only the
 gated `Source::markers`.
   - **The OCR run** (step 4, sheaf-app's ocr_run.rs). `flush(model,
 base, group, layers)` without `&OcrRun`; `page_by_page` returns
 `Landed { changes, requeue, failed, stopped }` (a Conflict queues
 the page, `EditError::Content` fails it, GroupEnded or another
 error stops the loop); `land()` is the batch with the page-by-page
 fallback on a Conflict or a Content error, which `flush_ocr` calls.
 `OcrRun.{ask, requeue, again}`; `Requeue { next, reading, tries }`
 (`push` stops after `REQUEUES` = 2; `take` gives the pages still
 there in document order and counts the dropped). `round()` builds a
 round's request and task; on Finished (not cancelled or undone)
 `next_round()` starts a new job and task over the queued pages on
 the current snapshot, in the run's group, and moves `base`. The
 message: "N pages recognized, M read again after a change"; a
 failed page is numbered by its place now, or in its round's base if
 it's gone. (The review moved the bookkeeping into `Tally`, below.)
   - **The plugin API** (step 5). WIT 0.3 (`wit/sheaf.wit`, still
 `sheaf:plugin@0.3.0`) lost `page-text-layer`; `recognize`, `text`
 and `render`'s docs say recognized text and a header, saved or not,
 are what the page draws now. `impl api::ocr::Host` left the
 include!d shared.rs: `api::shared::recognized(host, page, options)`
 holds `recognize`'s body, which v03.rs and v02.rs call, and v02.rs's
 `page_text_layer` answers None and records no read. The test
 plugin's `ocr` command reports `{words} by {engine}`. docs/plugins.md
 to match, with the 0.2 note and the Versioning line.
 `DocState::ocr_text`'s and blank.rs' "no file holds yet" wording, and
 step 1's docs that called a stack layer unsaved, say "saved or not".
   - **Corpus and interop** (step 6). append_corpus's ignored
 `ocr_round_trips` (`SHEAF_OCR_ONLY`, through `common::narrowed`):
 every page given one word (`SHEAFCORPUSOCR<n>`, `from_engine` in its
 view frame) a page at a time; the effects `CONTENT` without
 `REMOVES` or `REPLACES_TEXT`; an Append where the file takes one,
 the form check clean beyond what it was told, each word drawn once
 after a reopen; a header appended the same way, each page then two
 isolating streams and its word still drawn once; a re-OCR saved as
 planned, a Rewrite for `ReplacesText`, no old word in the bytes or
 the drawn text; a Clear the same. `header_trip`'s judge code is the
 shared `saved_flags`. interop.rs' `an_ocr_block_reads_in_pdfjs` and
 `_appended`.
   - **Gates, retired.rs, ledgers, docs** (step 7). gate_7k: on every
 fixture, recognized text set on every page in one transaction on
 the file opened afresh (each page's blocks read cold), the Append
 that writes it, its section and RSS, and its census
 (`Census::{ocr, ocr_not_one}`, `ocr_rows`, `assert_ocr`: one font,
 one block a page, asserted); `planned_save` (`planned_section`'s
 save, the file looked at before it's cut back); the history bench's
 first `SetOcrText` lowering, every page's blocks cold; the module
 doc's slice list. retired.rs' 13 rows and `add` qualified-only
 (`to_content` matched bare again; the `install` and `strip` reasons
 rewritten). The ledgers (above). docs/document-model.md's "OCR as
 content (M10.5 slice 18)", "Text layers (OCR)" rewritten, and the
 forward references (the origins, the namer's given names, emission,
 the effects, the removal check, the consumers table, OCR through
 `ViewPages`, the reading order, plugins and blank detection).
 README's rows name no legacy shape and were left.
   - **Deleted,** each confirmed dead by `cargo check --workspace
 --all-targets` and clippy, the names listed in retired.rs:
 `text_layer::{install, strip, has_layer, layer_entries, to_content,
 MARKER, FONT_NAME}` and their tests; `ContentStack::with_ocr_text`
 (both paths); `emit::content::fresh_balance`, the emitter's
 `replaced`/`text` plumbing and `page_res`' inline case and its
 `text` parameter; `ContentWriter::finish` and `named`'s `given`;
 `content::sink::Sink::{add, stream_dict}` and their impls (`array`
 stays: `content_entries` reads through it); the plugin host's
 `ocr::Host` impl in shared.rs, and with it 0.3's
 `page_text_layer` (`api::shared::HostState::page_text_layer`). `rg
 -n '/SheafOcr\b|"SheafOcr"|SheafOcrLayer' crates` finds the
 classifier (blocks.rs' `LEGACY_OCR` and its docs),
 `StreamSel::SheafOcr`'s doc, the frozen fixture and retired.rs
 only; `rg 'page_text_layer|page-text-layer'` outside plan/ finds
 WIT 0.2, v02.rs, host.rs's 0.2 test, docs/plugins.md's note and
 Versioning line, docs/document-model.md and retired.rs.

   Deviations from the plan (the planner's list first, as resolved):
   - **No `BlockKind`:** `Layer { id, body }`, the `Static` variant its
 kind. `Flattened` has no producer until slice 19's
 `FlattenAnnotations` (no unproduced variants); `BlockKind` comes
 with it.
   - **Two selectors only:** `StreamSel::SheafOcr` and
 `MarkedSel::SheafOcr`. §3.4's `StreamSel::SheafBlock(BlockId)` and
 `Isolation` have no producer.
   - **`/Sh…` names, not `/ShP<n>`:** `ContentWriter`'s namer names the
 list (`/Sh` and a digest), as slices 16 and 17's blocks are named.
   - **The block has a `q` of its own:** `/ShOCR <list> BDC q BT 3 Tr …
 ET Q EMC`, which §11.4 doesn't say; without it `3 Tr` reaches the
 generator blocks after it, which is why slice 17 put the legacy
 layer last.
   - **`EditError::Content { op, page, why }`** carries the
 `ContentError`'s text (`on_page`), not the error, whose sources
 aren't `Eq`; `EditError` stays `Clone + Eq`.
   - **Ids kept on replace:** a layer replacing the stack's, saved or
 not, keeps its id; a new one draws one. `BlockId::draw` takes
 `taken`, which `Lower::block_id` gives as the ids this op drew (no
 global scan); a pinned id is replayed as is.
   - **The merged-range check is `cut_together`:** `content::splice` on
 all the page's merged OCR ranges at once, which holds exactly when
 `splice_each` takes each, and gives the `SpliceError` for
 `ContentError::Unsafe`.
   - **`put_ocr` reads `Page(p)`** as `remove_generated` does, so a Set
 or Clear that changes nothing still conflicts under `Validate` with
 a concurrent change to the page (before, an empty layer always
 wrote, with the same conflict); an empty layer means none, so the
 tests that told versions apart by empty layers use one word.
   - **Re-queue rounds in the app,** not sheaf-ocr: the job takes a
 fixed list from one snapshot, so when a round finishes (not
 cancelled, undone or Esc'd after Finished) the queued pages still
 there start a new job on the current snapshot, in the run's group,
 with a new task and receiver, by `PageId` (`ReplacePages` keeps
 ids). Each page at most twice, then counted as changed; pages gone
 before their round and left when the run stops are counted as
 changed too. `Requeue` holds the bookkeeping so it's tested without
 GPUI; `Landed.stopped` lets `page_by_page` return the changes made
 before a stop with the stop. "read again", not "recognized again":
 a page read again can still be skipped or fail. A page that changed
 since its base and can't be read on it is failed, not re-queued
 (Content is lowered before Validate looks); OCR run again reads it.
   - **The plugin split for 0.2, no WIT bump:** `ocr::Host` is in
 v02.rs and v03.rs over `api::shared::recognized`; 0.2's
 `page-text-layer` answers none until slice 20 (a ledger row, 18 to
 20); 0.3.0 keeps its version though it lost a function (For Greg).
 The render interface's docs say "now … saved or not" as the text
 interface's do.
   - **Files saved with OCR alone are routed through `wrapped`'s
 `PageBlocks`:** `Source::markers` stays gated on `/SheafGenerators`;
 `stack`'s unmarked case reads the page's blocks (the read
 `wrapped` made, moved) and sends a page with sheaf's isolation or a
 stream-level block to `rebuilt`, which reads markers from what it
 decodes. `intact`, `untouched` and the render key stay on the gated
 read, so the base engine draws such a page when nothing is drawn on
 it, which is right.
   - **`intact` and `rebuilt` treat saved OCR as static-over content**
 after `QClose`; `rebuilt` leaves the kept blocks out of base′'s
 balance and puts back the names they draw with by one
 `names_used` pass over those streams, so the `QClose` count is
 exact even where another tool changed an OCR stream.
   - **Set fails on unreadable content** (§3.4: a base that can't be
 lexed fails the op that needs isolation), where the installer drew
 without lexing; Clear too, even over a stack layer alone. The app's
 flush counts such a page as failed and goes on.
   - **A saved layer's re-OCR plans a Rewrite from the effects**
 (`ReplacesText([OcrLayers n])`), not through the removal check's
 veto: kept::tests' `re_ocr_reopened` ports its reason;
 removal_tests' `a_re_ocr_over_a_saved_layer_is_a_rewrite` forces an
 Append, so its `ObjKey::Base` veto stands. History marks such a
 step as removing.
   - **A lone legacy layer stream stays and is counted,** in a Note of
 its own ("Recognized text sheaf wrote earlier stays on {pages}:
 taking it out would change what else the page draws.",
 `Cx.text_stayed`), not the stayed note, which names others'
 watermarks and headers. It's counted only where the page also holds
 saved text a filter takes out: lowering adds `DropStreams(SheafOcr)`
 for a block or a pair only (Found).
   - **The CID_FONTS version raise moved into `stack()`,** after a
 successful draw, so an unreadable page's text doesn't raise it;
 `needs()` stays for `needs_after`. `Sink::array` stays
 (`content_entries`); only `add` and `stream_dict` went, and
 `Cx::sym_dict` lost all but one caller and was inlined (no retired
 row: `Closure::sym_dict` is live under that name).
   - **Tests moved and merged:** the OCR tests of rewrite_tests.rs and
 materialize/tests.rs are materialize/ocr_tests.rs, save.rs
 importing its helpers (no retired rows for helpers that moved);
 `reinstall_replaces_previous_layer` and
 `a_re_ocr_over_a_base_layer_leaves_one_layer` became
 `replacing_a_saved_layer_rewrites_without_the_old_words`;
 `one_font_set_per_file` merged into `ocr_pages_share_one_font`;
 `empty_layer_writes_nothing` runs both ways. The reading-order test
 in sheaf-doc checks the streams' order, each stream's words through
 `text_bag` (a multiset); PDFium's order is sheaf-ops' text.rs's and
 the app's find test's.
   - **The PDFium word-box check of a saved block over an unbalanced
 base** is sheaf-ops' text.rs (`a_saved_block_over_an_unbalanced_
 base_stays_in_place`, both ways, through the text service under
 screen inputs before saving too): sheaf-doc's tests have no PDFium.
   - **The job-level unsaved-header check** is in sheaf-ops' realize.rs
 with its `Blind` engine: sheaf_ocr's `FakeSource` has no document to
 hold a header. The header is "Draft, for the board's review", over
 `HAS_TEXT_MIN_CHARS`, and a skip-pages-with-text job reads both
 pages.
   - **The corpus tier appends where the file takes one**
 (`appended_or_not`) for its adding saves, not `save()` as planned:
 planned, 893 files would Rewrite on the first save (817
 RemovalCheck, 46 BaseObjectsMissing, 30 BaseUnsound), 2 of them
 into the pre-existing form-check bug (Found). It asserts the
 effects instead (`CONTENT`, neither `REMOVES` nor `REPLACES_TEXT`).
 The form check runs after every save, the re-OCR and Clear Rewrites
 included (the review; it ran only after the adding saves before):
 the re-OCR's Rewrite hits that bug on the 8 files, so the tier fails
 at the pre-merge run as the watermark and header tiers do. Stronger
 than asked: the re-OCR and Clear must be Rewrites for
 `ReplacesText`, the re-OCR must give text to exactly the pages the
 first did, and each new word is drawn once. The header pass uses a
 single-slot header (`fx_header`): it's about isolation and the kept
 block, not numbering.
   - **The 7k census is of the Append's file,** read before it's cut
 back (`planned_save`), where slice 17's Bates census is of a
 Rewrite; and it doesn't assert a resources dictionary for each the
 file's pages had: each block names its own list, so each page has
 its own (7,000 lists added, printed).

   Found while building:
   - **A Rewrite of 8 corpus forms adds form-check findings** the
 original doesn't have (step 6; pre-existing at `bf7baf2`, built from
 a `git archive` export with its own target and deleted): 0019944
 and 0150632 `MissingFonts`, 0111785, 0622462, 1508501 and 2433684
 `OffPage`, 0092321 `WrongParent`, 0982696 `DuplicateNames`. Slice
 16's `watermark_round_trips` and slice 17's `header_round_trips`
 fail on these 8 at `bf7baf2` as now ("Rewritten: the form check
 finds …"), so the pre-merge run of those tiers fails regardless of
 this slice. This slice's `ocr_round_trips` fails on the same 8 and
 no other, at its re-OCR Rewrite, each with findings identical to
 the header and watermark tiers' on the same file (the review: the
 whole corpus, 131 s, and the two tiers narrowed to the 8, their
 failure lines compared). It reproduces without a generator: a 1 pt crop saved as
 planned (a Rewrite for RemovalCheck) on 0092321 and 0622462; a crop
 and two notes appended, then one note deleted (a Rewrite), on the
 other six. On 7 of them the findings are the original's, shorter,
 and the tiers' comparison was wrong; 0982696's was real. Both are
 fixed in the wrap-up, at the end.
   - **A generator removed from 0092321 left its text** (the wrap-up;
 fixed there): a field's `/Kids` holds the file's catalog, so a
 Rewrite copied the catalog's appended version, `/SheafGenerators`
 with it, as a field kid.
   - **A Clear over a page whose only legacy OCR stream lost its `q`**
 (no pair, no block) adds no filter: it changes nothing and says
 nothing.
   - **No app command calls `ClearOcrText`** (only tests, the API and
 fuzz); the slice asks for no UI.
   - **`cargo clippy -p sheaf-app --all-targets --no-default-features`**
 fails on `contrib.rs:158`'s `Contributions::add_panel`, dead without
 the plugins feature: pre-existing, and no CI configuration builds it.
   - **An OCR-every-page Append takes about 10 s at 7,000 pages, as it
 did at slice 17** (step 7's probe, below): it's the layer's
 operators, written a page at a time (0.8 ms for 300 words) and
 deflated (0.5 ms), not this slice's blocks; a later perf pass.

   The review (11 findings on the slice's changes, each checked against
   the code; 10 fixed, one fixed with no measurable gain):
   - **The OCR tier skipped the form check after its Rewrites** (the
 only saves on this slice's removal paths): `nothing_found` now runs
 after the re-OCR and the Clear too, as the watermark and header
 tiers run it. Over the whole corpus it finds exactly the 8 files of
 the pre-existing bug (Found), each with the header and watermark
 tiers' findings, line for line; no other file. Left to surface at
 the pre-merge run like those tiers' failures, not asserted against
 a list.
   - **The run's message counted dropped results as recognized** (three
 findings, one fix): `done` counted every result, and `again` took
 off only those read again, so a page past the cap, gone before its
 round, or left when the run stopped was counted both recognized
 and changed. A dropped result is now counted where it's dropped
 (`Tally.dropped`) and recognized is `done - dropped`: a page read
 again counts once, as its last read went, and one never read again
 only as changed; `again` is the message's clause alone, and the
 progress bar is unchanged.
   - **Nothing tested the run's wiring** (two findings, one fix): the
 event bookkeeping, the flush, the choice of the next round (its
 pages, base and total) and the message moved out of the view into
 `Tally`, which `Tally::take(ev, &mut Document, stopped) -> Taken {
 changes, notice, next: Next::{Wait, Round(ids), Over(msg),
 Aborted(msg)} }` drives; the view only starts the round's job
 (`round()` returns its handle and task) and follows the commits.
 Tests: `a_run_reads_a_replaced_page_again_in_its_next_round` (a
 ReplacePages during round 1, `Next::Round([p1])` on the new
 version, p1's second read landing in the run's one version, the
 message "3 pages recognized, 1 read again after a change") and
 `a_page_not_read_again_counts_as_changed` (stopped, gone, and the
 cap: "2 pages recognized, 1 changed …" twice and "0 pages
 recognized, 2 read again after a change, 1 changed …"). Not a
 headless GPUI run: `sheaf_ocr::job::start` takes no engine, and the
 flush's timing against a commit would be the test's. Deviation: a
 run whose document is opened again (no longer Ready) is dropped at
 its next event, its job cancelled, where it used to linger to
 Finished with its flushes skipped.
   - **"Skip pages with text" depended on whether the text was saved**
 (`ViewPages::text_len` under `content_only`, which keeps a saved
 block but not the stack's layer): it now reads with `ocr_over`, as
 blank detection does, and pixels stay `content_only`. Test:
 realize.rs' `ocr_skips_a_page_recognized_before_saved_or_not` (a
 page given 33 characters of text, skipped unsaved and on the file
 saved and opened again, both read alike); it fails with the old
 inputs. document-model.md's consumer tables and Text layers say so.
   - **"Unsaved header" wording** where `content_only` sets aside sheaf's
 header saved or not: the WIT's `recognize` (0.3 only; `wit/0.2` is
 frozen), docs/plugins.md's `ocr`, `RenderSource`, the app's
 `EngineRender`, `ViewPages`, `content_only` and blank.rs's module
 doc; document-model.md's flush paragraph names a ReplacePages as the
 Conflict, not slice 17's deleted header strip.
   - **`put_ocr` built a second stack** for `with_filters` with no
 filters, and looked its page up three times: it skips
 `with_filters` when there are none and passes its one `def` to
 `saved_ocr`. Output identical (the same stack either way). The
 history row's 7,000 lowered `SetOcrText`: 48.3 ms in the gate run
 after, five runs 48.0–52.1 ms (median 48.8 ms) against step 7's
 48.4 ms, so not the cause of the rise from 38 ms: that's the page's
 blocks read for the filters (`saved_ocr`), which correctness needs.
   - **Fixture builders copied** in tx/ocr_tests.rs and ocr_run.rs'
 tests: one `text_layer::fixture::pages_drawing(contents) -> Pdf`
 (the `fixtures` feature), on the same `with_pages` as `one_page`;
 the fixtures' bytes as before. watermark.rs' `pages_pdf` (slice
 16's, with a font and a box on each page) stays.
   - **Comments re-wrapped** where the slice left them long or ragged
 (emit/content.rs, source/mod.rs, untouched.rs, tx/content.rs,
 watermark.rs, text.rs, blank.rs).

   Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
   - **The gates** (`gate_7k`, release, each test in a run of its own,
 each started once the 1-minute load was under 4 on two checks 30 s
 apart: load 1.4 to 2.3 at each start; 11 of 11 pass):

 | Row | Synthetic | Heavy | Scanned |
 |---|---|---|---|
 | recognized text on every page (300 words each), one transaction on the file opened afresh, each page's blocks read cold | 197 ms | 268 ms | 141 ms |
 | the Append that writes it, planned and written; RSS added; the section | 9.96 s, 186 MB, 17.5 MB | 9.97 s, 245 MB, 17.4 MB | 10.06 s, 196 MB, 17.6 MB |
 | its census: fonts added (asserted 1), blocks (asserted 7,000, one a page), property lists added | 1, 7,000, 7,000 | 1, 7,000, 7,000 | 1, 7,000, 7,000 |
 | realize + open + render 1,024 px, an edited page, a header and recognized text on it (asserted < 15 and < 25 ms) | 12.0 ms (slice 17: 12.0) | 22.7 ms (21.2–21.8) | 32.2 ms (31.8) |
 | realize an edited page (emit + assemble), fresh | 2.03 ms | 2.49 ms | 2.03 ms |
 | thumbnail strip, crop + watermark + "Page n of N" header, 320 px (asserted ≥ 310 pages/s, ≤ 96 MB weighed and added) | 417 pages/s, 80.7 MB, 25.8 MB | 352 pages/s, 80.7 MB, 60.9 MB | 164 pages/s, 80.8 MB, 80.2 MB |
 | crop all pages, and validated after a delete (asserted < 200 ms) | 13.5, 15.4 ms | 10.1, 15.8 ms | 9.49, 14.7 ms |
 | Bates apply, all pages (asserted < 50 ms), and the fonts its Rewrite adds (asserted 1) | 0.617 ms, 1 | 0.638 ms, 1 | 0.625 ms, 1 |
 | census of a watermark and a PNG background (asserted 1, 1, 1, 0; page resources) | 1, 1, 1, 0; 1 of 1 | 1, 1, 1, 0; 70 of 70 | 1, 1, 1, 0; 7,000 of 7,000 |
 | engines after 220 groups: weighed, RSS added, open (asserted ≤ 96 MB, ≤ 96 MB, ≤ 6) | 28.9 MB, 41.5 MB, 6 | 28.9 MB, 65.3 MB, 6 | 29.0 MB, 61.8 MB, 6 |
 | one-mark Append on the scanned (asserted < 50 ms) | | | 19.3 ms |
 | snapshot, `Tx::check` of a delete of every page (asserted < 50 µs, < 1 ms) | 0.241 µs, 0.393 ms | 0.281 µs, 0.395 ms | 0.251 µs, 0.389 ms |
 | geometry walk (asserted < 100 and < 250 ms); ready (asserted < 1 s on the heavy) | 48.1 ms; 54.8 ms | 79.1 ms; 357 ms | 60.6 ms; 440 ms |
 | "Page n of N", saved, every page moved: the Append (asserted ≤ 1 s), the section (asserted ≤ 8 MB) | 469 ms, 1.36 MB | 619 ms, 1.47 MB | 474 ms, 1.37 MB |
 | the same move on the file opened again: the Append (asserted ≤ 1 s on the scanned; accepted misses on the others), the section (asserted ≤ 8 MB) | 888 ms, 3.31 MB | 1,555 ms, 3.34 MB | 888 ms, 3.32 MB |
 | proposal 8's draws: set unsaved, a moved page, one deleted, one deleted on the file opened again (accepted misses on the synthetic and heavy) | 22.4, 20.5, 18.7, 29.4 ms | 39.1, 34.9, 32.2, 42.6 ms | 33.5, 32.8, 31.2, 42.7 ms |

 Proposal 8's accepted misses are where slice 17 left them, within
 its runs' spread (slice 17: 22.3, 18.8–20.3, 19.3 and 27.3 ms on
 the synthetic, 36.7–39.7, 33.9–35.4, 31.8 and 41.8 ms on the heavy,
 the Appends on the file opened again 0.89–1.07 s and 1.43–1.57 s);
 none of those paths draws recognized text. The rest: history's 200
 OCR versions of 1,000 pages at most 236 MB (≤ 256 MB, asserted;
 slice 17: 236); AddMark 0.020 ms and a full reorder 0.950 ms at
 20,000 marks (< 1 and < 2 ms); 7,000 `SetOcrText` in one
 transaction lowered in 48.4 ms, the first 67.0 ms with every blank
 page's blocks read cold (38 ms before this slice's read), committed
 in 13.5 ms, an OCR run of 700 flushes 108 ms; 1,000 mark adds on the
 heavy 0.038 ms (blank 0.023 ms); ready on 20,000 sheaf marks 522 ms
 (< 1 s); their Rewrite 1,523 ms; Trim White 46.4, 40.2 and 95.4 s
 (no gate; slice 17 38.7–41.6, 42.6–42.8 and 100.8–105.5 s); Find
 Blank Pages over the synthetic 12.4 s, and Find with a header on
 every page past six pinned groups 12.2 and 9.1 s (no gate).
   - **The Rewrite against slice 8's writer**
 (`rewrites_on_the_synthetic_and_heavy_7k` five times, each fixture
 in a process of its own, against slice 8's 134 and 707 ms):
 synthetic 146–159 ms, median 149 ms, 1.11× (1.09–1.19×); heavy
 760–778 ms, median 776 ms, 1.10× (1.07–1.10×); none past the 1.2×
 flag (slice 17: 1.12× and 1.10×). The gates' own Rewrite parts: 146
 ms (1.09×) and 814 ms (1.15×). RSS added 51.0 and 253 MB (≤ 1.5×
 slice 8's 61.7 and 251 MB).
   - **The OCR-every-page Append against slice 17's** (a scratch probe,
 the same code in both builds, slice 17's from a `git archive` of
 `bf7baf2` with a target of its own, deleted after; each fixture's
 work copy in the scratch directory; one run each, load 2.1 to 3.9):
 the transaction 46.2, 48.3 and 46.1 ms at slice 17, 212, 279 and
 140 ms now (every page's blocks read cold, the plan's risk); the
 Append 9.46, 9.76 and 9.42 s at slice 17, 9.87, 10.17 and 9.90 s
 now, its section made in memory 9.42–9.66 s and 9.86–9.99 s; the
 section 1.46–1.59 MB at slice 17 and 17.4–17.6 MB now. The section
 grew because the gate gives every page the same layer: slice 17's
 installer wrote identical layers as one object, and each layer has
 its own id now, so its own block, list and `PageRes` (§4, the plan's
 risk); real OCR differs page by page, so slice 17 wrote one stream
 a page there too. A probe of `text_layer::block` alone (a scratch
 test in sheaf-doc, release, removed and the file checked with
 `cmp`): 7,000 pages of the gate's 300-word layer in 5.59 s (175 MB of
 operators) and deflated in 3.67 s, which is the Append's time.
   - **Proposal 8's rows A/B** (the synthetic's gates, slice 17's build
 and this slice's in turn, twice each, on the same fixtures, load
 2.1 to 2.9 at each start): set unsaved 22.3, 22.5 ms at slice 17
 and 22.5, 22.9 ms now; a moved page 20.2, 20.0 and 20.3, 18.8 ms;
 one deleted 19.3, 18.2 and 18.5, 19.6 ms; one deleted on the file
 opened again 29.1, 29.4 and 29.3, 30.0 ms; the reorder's Append on
 the file opened again 1,167, 854 and 1,067, 1,072 ms; realize +
 open + render 11.9, 11.9 and 12.5, 12.0 ms. No worse. A second run
 of the heavy's gates (load up to 5.1 at its end): realize + open +
 render 21.6 ms, proposal 8's draws 38.1, 34.7, 31.5 and 42.4 ms, the
 Append on the file opened again 1,489 ms, the OCR Append 10.0 s.
   - **The corpus** (through the scratch root; no "skipped: no corpus"
 line), every count as slice 17 left it: materialize_corpus's
 identity 4,389 of 4,392 written, 2,269,722 primary objects
 surviving, 919,479 copied as they were, 436 changed by `keep_only`,
 0 failed; 2,901 forms without their first page, 0 failed; every page
 duplicated, 4,389, 0 failed; inserted twice into the 1040, 1,098, 0
 failed; four times, 0 differ; the logo, 0 of 2,037,600 bytes differ.
 append_corpus: 4,389 marked, 3,770 appended, 259 rewritten (211
 RemovalCheck, 34 BaseObjectsMissing, 14 BaseUnsound), 360 asked, 0
 failed, 0 derived vetoes and 211 on the files' own objects; the
 oracle finds text gone from 2,899 of 2,901 (none from 0016064 and
 0112654). encrypted_corpus: the Rewrite keeps 573 of 573 and the
 Append 572 (0630899 declined). The OCR tier (`SHEAF_OCR_ONLY`) on
 six files (the 1040, 0025448, 0187215 signed and PDF/A, 0861328,
 0442193 Word's, 0630899): every file recognized, given a header,
 recognized again and cleared, 11 of 12 adding saves appended (one a
 Rewrite for BaseUnsound), 0 pages unreadable or showing nothing, 0
 failed, 0 derived vetoes; on the whole corpus (step 6): 4,389 files,
 8,702 of 8,778 adding saves appended (Rewrites: 46
 BaseObjectsMissing, 30 BaseUnsound), 0 unreadable pages, 0 failed,
 0 derived vetoes, 145 s; with the form check after every save (the
 review): 4,389 files, 8,686 of 8,762 adding saves appended (the 8
 failing files' 16 not counted), 0 unreadable, 8 failed (Found's 8),
 0 derived vetoes, 131 s. The full ignored tiers (deleted means
 deleted, realize_corpus, the watermark, header and OCR tiers) are
 the pre-merge run's (Greg, 2026-10-20); the watermark, header and
 OCR tiers fail there on 8 files for a reason older than this slice
 (Found).
   - **Suites:** `cargo test --workspace` 1,575 passed and 48 ignored
 (slice 17: 1,544 and 45; ignored +3, the OCR tier and the pdf.js
 pair); after the review 1,578 and 48 (the two run tests and the
 skip test). The release properties: Append ≡ Rewrite at 1,024 cases and
 the replay property at 4,096 pass (67.5 s together). The interop
 tier (qpdf; pdf.js 6.4.299 on node 24.22): 15 pass with
 `--include-ignored` (slice 17: 13), `qpdf --check` clean on all 84
 outputs. fmt and clippy (`-D warnings`, all targets) clean, and
 clippy with sheaf-app's Linux-only gates switched off
 (`nonlinux-clippy.sh`); clippy (`-D warnings`) on
 `x86_64-pc-windows-msvc` passes for sheaf-doc, sheaf-edit,
 sheaf-util and sheaf-image with their tests and for sheaf-render,
 sheaf-print, sheaf-ops and sheaf-ocr with all targets; sheaf-plugin
 can't be checked for Windows here (wasmtime's fiber needs
 `windows.h` and ring `lib.exe`), as at slice 17, and its changes are
 platform-free.

   Checked by tests:
   - **Finding and saving:** OCR, then Find, hits words before saving
 (sheaf-ops' text.rs search test and the app's find test, slice
 17's hunks reversed: the page's text, then "Total 42", then
 "Draft"); `ocr_appends_one_block_a_page_and_one_font` (one
 `/SheafBlock /K /Ocr` stream a page, the isolation shared, one
 `GlyphLessFont` Type0), and the 7k census (one font, one block a
 page, on every fixture); `empty_layer_writes_nothing` both ways (no
 version, one content entry, no font, a planned save Unchanged).
   - **Removing saved layers:**
 `replacing_a_saved_layer_rewrites_without_the_old_words` over
 `legacy_page()`, `block_page()` and a file this writer saved and
 reopened (a Rewrite with `ReplacesText([OcrLayers 1])`, the old
 words in no decoded stream or string); `clear_removes_a_saved_layer`
 over those and `merged_page()` (the words gone, the merged stream
 byte for byte outside the cut, the block's names out of its
 resources, a Rewrite); `removing_a_legacy_layer_keeps_q_and_q_
 balanced` both ways (after a Clear, a re-OCR and a Clear with a
 header, the written content's balance is the base's own and the
 isolation counts `isolated()`'s); `a_legacy_layer_without_its_q_
 stays`; `a_page_that_cant_be_read_is_saved_without_its_text`; the
 tier on the corpus; pdf.js reading no recognized word after a
 Clear.
   - **Kept in their slot:** `a_reopened_ocr_page_is_intact` both ways
 (intact and untouched under screen inputs, `untouched` equal to
 `emission_is_base` under every input, a rewrite keeping the same
 contents; with a header and alone); `nesting_never_grows` both ways
 (five cycles from `block_page()`'s unbalanced base, the header
 changed each time: `q`, BASE, `Q Q`, the same block, the header,
 depth 2, balanced); `an_ocr_only_file_gets_a_header_later` both
 ways; `an_append_over_a_saved_block_vetoes_nothing`; sheaf-ops'
 `a_saved_block_over_an_unbalanced_base_stays_in_place` both ways
 (PDFium word boxes within 1/16 pt); blocks_tests' `Marker::of` on
 every stream.
   - **Lowering** (tx/ocr_tests.rs): the filters Set and Clear add over
 each fixture, replaced text counted once, one filter a page set or
 cleared again; a stack layer cleared back to the shared empty
 stack; Clear on nothing and an empty layer on a page with no text
 make no version; an unreadable page and an unsafe merged block give
 `EditError::Content` naming the page and change nothing; block ids
 in range, kept by a second Set, exact through undo and redo, shared
 by a duplicate, new after Clear then Set, pinned on Relower.
 effects' `saved_recognized_text_filtered` and the samples for the
 new filters and a same-text layer with another id.
   - **During a run:** ocr_run's `a_crop_and_a_turn_during_the_run_land`
 (one commit, no conflict, PageSpace rects unchanged), group_tests'
 `an_ocr_flush_lands_after_a_turn_of_its_page` beside the crop's,
 sheaf-ops crop.rs' `ocr_words_land_after_a_rotate_during_the_run`
 both ways (PDFium finds each word within 2 pt of its box);
 `a_replaced_page_is_recognized_again` and
 `a_page_is_recognized_again_twice_at_most` (the ReplacePages
 conflict itself is still key_tests'); realize.rs'
 `ocr_reads_a_turned_page_as_it_shows_its_content_alone`, now with
 the job's skip check; `a_page_that_can_t_be_read_fails_and_the_rest_
 land`; from the review, the run's `Tally` driven by events
 (`a_run_reads_a_replaced_page_again_in_its_next_round`,
 `a_page_not_read_again_counts_as_changed`) and realize.rs'
 `ocr_skips_a_page_recognized_before_saved_or_not`.
   - **Undo:** ocr_run's `a_flush_after_undo_ends_the_run`, and
 group_tests' GroupEnded after undo, one "Recognize text" version
 and "Undo all".
   - **The plugin API:** host.rs' `api_02_s_page_text_layer_answers_none`
 (the frozen 0.2 fixture's `ocr` command over a page with a layer:
 "page0 by fake-deu stored=false").
   - **Interop and corpus:** `an_ocr_block_reads_in_pdfjs(_appended)`
 (`qpdf --check` clean; pdf.js reads the page's text, then `begin
 ShOCR` "Recognized" "words" `end`, then the header's `/Artifact`;
 after a Clear and a planned Rewrite, the page's text and the header
 alone); the OCR tier.
   - **Fault checks** (each file restored from the step's own copy and
 checked with `cmp`; no proptest seed added): step 2's saved OCR
 streams not set aside, and the merged OCR cuts skipped (both caught);
 step 3's OCR marker not recognized (7 tests fail), routing to
 `rebuilt` off (4), `intact` ignoring saved text with no generator
 (2), the kept text's names dropped from the resources (5), and saved
 text left inside base′ (5 sheaf-doc tests and both PDFium tests);
 step 4's `page_by_page` not queueing and the cap loosened to
 `REQUEUES + 1` (both rounds tests fail); step 7's retired.rs rows,
 each of the 13 hit by a probe file defining it at its path and
 naming it, the qualified-only `install`, `strip` and `add` not hit
 when bare, and `to_content` hit bare (a package cargo doesn't build,
 deleted). The probes of step 7 (the OCR Append, `text_layer::block`'s
 cost) were scratch code, removed and the files checked with `cmp`.
 The review's: `Tally::next_round` not moving `base` (the run test
 fails), `dropped` not counted (both run tests), no next round (both),
 and the skip check's text read without `ocr_over` (the skip test);
 each restored from the fixer's own copy and checked with `cmp`.

   For Greg:
   - **WIT 0.3.0 lost `ocr.page-text-layer` without a version bump:**
 0.3 is unreleased and no plugin in the repository imports it, but a
 plugin built against 0.3.0 that did would fail to link. docs/
 plugins.md's Versioning says so. 0.2 keeps it, answering none, for
 the frozen fixture wasm, until slice 20.
   - **Set and Clear fail on a page whose content can't be read**
 (`EditError::Content`), where the legacy installer drew without
 lexing; an OCR run counts such a page as failed and goes on. The
 corpus has none (0 unreadable pages in the tier).
   - **A re-OCR or a Clear over a saved layer is a Rewrite** with the old
 words gone (`ReplacesText`), planned from the effects; History marks
 the step as removing.
   - **The pre-existing form-check findings on 8 corpus forms after a
 Rewrite** (Found): on 7 the tiers' comparison was wrong, and
 0982696's was a real defect, both fixed in the wrap-up below (the
 second at your word, reversing slice 8's keep). 0092321, once past
 the check, kept a removed generator's text through a stray catalog
 kid, also fixed at your word (the wrap-up).
   - **Wording, yours:** the run's ", 2 read again after a change"; the
 Notes "No recognized text, watermark, background, header or footer
 is drawn on page 3, whose content can't be read." and "Recognized
 text sheaf wrote earlier stays on page 3: taking it out would change
 what else the page draws."; `EditError::Content`'s "set-ocr-text:
 page 3's content can't be read; nothing was changed".
   - **An OCR-every-page save of 7,000 pages takes about 10 s** (the
 Append, on every fixture), as it did at slice 17 (9.4–9.8 s, the same
 probe): writing the layer's operators (0.8 ms a page for 300 words)
 and deflating them (0.5 ms). Its section is 17.5 MB, where slice
 17's gate-shaped run wrote 1.5 MB: identical layers were one object
 there, and each layer has its own id now. Not gated; a later perf
 pass could write and deflate pages in parallel.

   Left for later slices:
   - **Slice 19:** RemovePrivateData of `/SheafOcr` and `/SheafOcrLayer`
 in old files and stripping `/SheafOcrBlock` from the lists sheaf
 writes; `BlockKind` with `Flattened` and `Static::Placements`
 (`FlattenAnnotations`).
   - **Slice 20:** WIT 0.2's `page-text-layer` (a ledger row) goes with
 0.2; OCR in plugin API 0.4 with its permissions.
   - **The corpus's other collections:** `signatures/` (531 files) and
 `conformance/` (3,524) are read by no test yet (the layout and
 `scans/` are done, in the wrap-up below).
   - **Follow-ups:** the `Closure::stale` probe (slice 11's Unprobed),
 untouched: no step changed that walk. A Clear over a lone legacy
 layer stream (Found).
   - **A later perf pass:** an OCR'd page's block written and deflated
 page by page on one thread (about 1.3 ms a page, 10 s at 7,000);
 each OCR'd page's own `PageRes` and list; slices 16 and 17's items.

   Earlier slices' "Slice 18" bullets, as met:
   - **Slice 6's** (OCR's legacy shape, `/SheafOcrLayer` and
 `/SheafOcr`; the namer's given name `/SheafOcr` to 18): no layer is
 written in that shape and no name is given; `/SheafOcrLayer` is
 read only, to find an older sheaf's layer (`PageBlocks::legacy_ocr`)
 and take it out.
   - **Slice 8's** (`Layer` ids and kinds, OCR as blocks): `Layer.id` is
 a `BlockId`; the kind is the body's variant until slice 19 brings
 `Flattened` (Deviations).
   - **Slice 9's** (OCR as blocks, not the legacy shape): done, the
 ledger row (9 to 18) deleted.
   - **Slices 11, 16 and 17's** `EditError::Content`: SetOcrText and
 ClearOcrText map a `ContentError` to it.
   - **Slice 17's** (the OCR layer before the generators, the reading
 order, `page_res`' inline case, the installers kept qualified-only):
 done; the ledger row (17 to 18) deleted, and retired.rs lists the
 installers.

   Wrap-up: the corpus layout and the scans collection (Greg,
   2026-10-22, after the review):
   - **The corpus's new layout.** The main checkout's unfinished change,
 ported: `Corpus::forms()` (was `manifest()`) reads
 `forms/manifest.tsv`, realize_corpus reads the pool's
 `index/safedocs-forms-inventory.tsv.gz`, and a folder that's there
 without `forms/manifest.tsv` fails the corpus tests with an error
 naming it, where it made every one skip; `bench/real.py` stops the
 same way. Every run is on `../pdf-corpus` itself, and the scratch
 root of links is retired. On it, every count is as slice 17 left
 it but the identity Rewrite's, which the two form fixes below move:
 materialize_corpus 4,389 of 4,392 written (2,269,688 survived, from
 2,269,722, 919,465 copied as they were, from 919,479, and 432
 changed by keep_only, from 436: 0982696's and 0092321's alone),
 2,901 without their first
 page, 4,389 duplicated, 1,098 inserted twice, 4,389 written four
 times with 0 differing, the logo's 0 of 2,037,600 bytes;
 append_corpus 4,389 marked, 3,770 appended, 259 rewritten (211
 RemovalCheck, 34 BaseObjectsMissing, 14 BaseUnsound), 360 asked, 0
 derived vetoes, the oracle's text gone from 2,899 of 2,901;
 encrypted_corpus's Rewrite keeping 573 of 573 and its Append 572
 (0630899 declined).
   - **Its scans** (`scans/`, 709 files: 334 scans, 260 with another
 program's recognized text, 70 with visible text, 45 only looking
 like scans; CCITT, JBIG2 and JPX among their images):
 `Corpus::scans()`, and three ignored passes over them, narrowed by
 `SHEAF_OCR_ONLY` and `SHEAF_REALIZE_ONLY`.
 - `ocr_round_trips_scans`: all 709 recognized, headed, recognized
   again and cleared, 1,416 of the 1,418 saves adding text or a
   header appended (2 Rewrites for BaseUnsound), 0 failed, 0 derived
   vetoes. On the 260 with another program's text, every word and
   string the file drew before (`TextBag::missing_from`: the words,
   and by hash the strings in codes a font doesn't say the text of,
   as 10 of them draw theirs, in Identity-H fonts with no
   `/ToUnicode`) is still drawn, as often, with ours added and with
   ours cleared: 884,144 in all. Fault check: their words measured
   with ours in fail at the Clear.
 - realize_corpus `the_predicate_agrees_on_every_scan` and
   `scans_render_as_saved`, each scan's edited pages also given
   recognized text (1,498 took it): the model disagrees on 0 of
   28,485 pages and inputs, and 0 of 1,769 pages differ.
   - **The tiers' form check compares by what each finding names**
 (slice 15's crop tier compared whole findings, and 16–18 copied it,
 so a finding a Rewrite made shorter counted as new):
 `Report::added_since`, beside `Finding::items` (moved from
 verify.rs), gives what names an item, or names it more often, that
 the input's finding of the kind doesn't, and a font missing for
 more widgets than it was; any other kind as before. One
 `nothing_found` serves the crop, watermark, header and OCR tiers
 (the crop tier still leaves nothing a save told of). A unit test,
 fault-checked three ways (the kind, the count, the multiset).
 With it alone, the crop tier passes the 8 files and the OCR tier
 fails only 0982696, over the 8 and over the whole corpus. Two were
 real:
 - **0982696's 4 `DuplicateNames`** (`…Page1[0].Table6[2].Row3[0]`,
   `Row4[0]` to `Row4[2]`): a node whose kids another node holds,
   written as a typeless terminal field. Fixed (Greg, 2026-10-22:
   reverses slice 8's keep of a node whose kids another holds; it was
   written as a typeless field): `Fields::field` (emit/form.rs) writes
   no node that isn't a terminal and has nothing under it written.
   Slice 8's `a_node_whose_kids_another_holds_stays` is
   `…_goes` (the written names `other`, `real` and `real.kept`, the
   form check clean; fault-checked). 0982696's identity Rewrite has 20
   duplicate names, all the original's 24's, and writes 19 objects
   fewer (the identity baseline, above); no other file's counts move.
   With it, the crop and OCR tiers pass the 8 files, and the
   watermark and header tiers all but 0092321; over the whole corpus
   the OCR tier passes (4,389 files, 8,702 of the 8,778 saves adding
   text or a header appended, 0 failed).
 - **0092321 kept a removed watermark's or header's text** (the
   watermark and header tiers' Appended way, past the form check
   now): field `Page2[0]`'s `/Kids` holds the catalog (object 232,
   the file's own `WrongParent`), so the Rewrite that took the
   generator off wrote a new catalog but copied 232's appended
   version, `/SheafGenerators` and the removed spec's text with it,
   as a field kid. Fixed (Greg, 2026-10-22): of a primary field's
   kids nothing accounts for, `Fields::read_from` (emit/form.rs)
   copies only widget annotations (`/Subtype /Widget`), a page
   template's; a page, the catalog or anything else isn't the field's.
   Tests: `a_watermark_taken_off_isn_t_kept_by_a_field_s_stray_kid`
   (watermark_tests; 0092321's pattern, the file opened again after
   the Append) and `a_field_s_widget_on_a_page_template_stays_its_kid`
   (annot_form_tests), each failing with the filter undone (dropped,
   or taking every kid). 0092321's identity Rewrite writes 15 objects
   fewer: its two catalogs copied as kids, and the usage rights
   (`/Perms`, `/UR3`) only they reached, which a Rewrite's own
   catalog drops (the identity baseline, above). Narrowed to the 8
   files, the crop, watermark, header and OCR tiers all pass.
