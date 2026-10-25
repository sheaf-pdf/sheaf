# M10.5 slice 17: Header/footer and Bates; derived sources gone

Status: done · Part of [M10.5: the edit API](README.md)

**Header/footer and Bates; derived sources gone** (L; proposals 8
and 31; bugs 16, 24, 28 and 29).
- **The header/footer generator:**
  - live page tokens with `needs()`;
  - frozen Bates numbers persisted per block (`/N`);
  - frozen date and file;
  - `Scope::All`.
- **Live redraws:** `LIVE_REDRAW` via `SaveRecord.drawn` (layout
  against recipe digests), and allowed drops in the removal check.
- **`RemoveHeaderFooter`** by origin.
- **Dialog prefill** from the model or a recognized generator.
- **Then the last users of derived sources go.**
- **Deletes:**
  - `stamps::install` and `to_catalog`/`from_catalog`;
  - derive::strip_stamps and `stamps::strip`, the old Remove
    Header/Footer (derive.rs:39, page_insert.rs:428);
  - `/SheafHeaderFooter` and `/SheafHF` (read only);
  - the legacy header/footer drawing and `Why::LegacyHeaderFooter`;
  - `SetPageBase` (header/footer strips were its last use) and the
    `RebasePages` remnant;
  - the `Derived` side record and `MemoryKind::Derived`;
  - `derived_copy` and `copy_form`;
  - derived-page emission in the emitter and the realizer;
  - sheaf-ops derive.rs;
  - page_insert.rs's derived flows (about 350 lines).
- **Checks:**
  - **Live tokens:**
    - a reorder after a save renumbers "Page n of N", the save
      appends (`LIVE_REDRAW`), and the removal check passes;
    - an extract of an unsaved document keeps the binder's Bates
      numbers and renumbers page tokens.
  - **Frozen Bates:** Bates numbers don't change on a reorder, and
    survive a save plus reopen (rebuilt from `/N`).
  - **Applying:**
    - re-applying identical settings is a no-op;
    - with `Scope::All`, a page inserted later gets the header.
  - **Removal:**
    - changing the header text after a save gives a Rewrite with the
      old text absent;
    - a Word-made PDF's running heads are left by `SetHeaderFooter`
      and counted by `RemoveHeaderFooter`.
  - **Recovery** works with an unsaved header.
  - **No derived sources:**
    - a grep finds no derived-source code;
    - the forms-survive-page-tools tests (M10 slices 3–5) pass
      without copies.
  - **Speed:** a 7,000-page Bates applies in under 50 ms, with one
    font object.
  - **Gate, proposal 8's verdict:** after a save, reordering all
    pages of the heavy 7k under a "Page n of N" header gives an
    Append of at most 8 MB, planned and written in at most 1 s, and
    each visible page re-renders within the realize gate. The
    result goes in the slice notes as proposal 8's verdict; a miss
    means frozen numbers plus Update. (Greg kept the numbers live
    and accepted the misses, 2026-10-22: proposal 8's verdict.)
  - **Unencodable labels:** a label change that makes `<<label>>`
    unencodable draws `'?'` with a notice, and saving still works.

## Notes

Done. Header/footer and Bates; derived sources gone (proposals 8 and
   31; bugs 16, 24, 28 and 29): the header and footer is a §3.5 generator,
   set and taken off by ops, its page tokens (`<<1>>`, `<<N>>` and a new
   `<<label>>`) live and its Bates numbers, date and file fixed when it's
   set, the numbers kept in each block's `/N`. It's drawn as `/SheafBlock`
   blocks when a save writes a page or a view draws it, found again from
   `/SheafGenerators` when the file opens, and drawn again after a reorder
   by an Append (`LIVE_REDRAW`, `SaveRecord.drawn`, allowed drops). Remove
   Header/Footer takes others' headers by origin with `include_authored`,
   and the Watermark and Header & Footer dialogs show what applying would
   replace and Remove would take, by who drew it, counted off the UI
   thread. Then derived sources went: derive.rs, stamps.rs, the `Derived`
   record and `MemoryKind::Derived`, `derived_copy` and `copy_form`,
   `SetPageBase` and `Derive`, and the emitter's and the realizer's
   derived pages. Bates numbers on 7,000 pages apply in 0.62–0.64 ms with
   one font object in the Rewrite (asserted < 50 ms and 1). Proposal 8's
   verdict: page numbers stay live (Greg, 2026-10-22), not the fallback.
   The reorder's Append passes in the session that saved the header on
   every fixture (asserted ≤ 1 s and ≤ 8 MB) and on the scanned opened
   again; on the synthetic and heavy opened again, where every block is
   written anew, it misses (1.57 s on the heavy, 0.89–1.07 s on the
   synthetic over two runs), and the first page drawn of a group every
   page of which is laid out anew misses the realize gate (19–27 ms on the
   synthetic against 15, 32–42 ms on the heavy against 25), after the
   follow-up's speedups (a header laid out once a page for every version;
   deltas without the frame). Greg accepted the misses: they're measured,
   and asserted only with `SHEAF_GATE_PROPOSAL_8`; a lead window (option
   A) is to revisit. The corpus counts hold.
   - **The model** (step 1, sheaf-edit and sheaf-doc). sheaf-edit:
 `HeaderFooterSpec.font: FontChoice` (`#[serde(default =
 "legacy_face")]`, Helvetica, the legacy face, where `FontChoice`'s
 own default is HelveticaBold), `<<label>>` documented in `texts`,
 the `every_op` fixtures with fonts (one using `<<label>>`),
 `schema/ops-1.0.json` regenerated (additive: `font`, descriptions).
 records.rs: `Generator { id, spec, z, scope, frozen, replaces }`,
 `Frozen { date, file, bates: Option<Arc<im::OrdMap<PageId, u64>>> }`
 with `bates_number(page)`, `GenSpec::{HeaderFooter(Arc<
 HeaderFooterSpec>), Watermark(Arc<WatermarkGen>)}` (`WatermarkGen`
 its content and position), `CtxNeeds.label` and `order()`,
 `PageCtx { geom, position, count, label, bates }`,
 `Generator::same_place` (kind and z), and `on_pages_left` alike for
 every generator, dropping the numbers of pages that go under any
 scope (the map cloned, the entries removed, the rest shared) and
 counting them, so the cascade puts a generator whose numbers alone
 changed. The new `generate/header.rs`: `TOKENS` (six), `Line` (was
 `stamps::Placed`), `kinds`, `is_empty`, `unfit`, `needs`,
 `bates_text`, `numbered` (None past `u64::MAX`, which lowering
 refuses: the review's), `slots`, `unencodable`, `label_unencodable`
 and `layout(spec, frozen, ctx, face)`;
 `generate::page_ctx(st, g, id, pos, geom)` reading the label only for
 `<<label>>`. `DocState::header_footer() -> Option<&Arc<Generator>>`,
 `label_of(id)` cached in `Indexes.labels` (its number with no
 labels), `snapshot_bytes` weighing that cache only when a generator
 reads labels, `check_invariants` (one header, a non-empty `Pages`
 scope of real pages, Bates keys pages it covers) and `resolves_after`
 naming the numbered pages. tx: `set_header_footer` (`Result`; the
 replaced generator's id, date and file kept, else the host's; Bates
 counted over the covered pages in order, reading `Order`; z `Over`,
 `replaces` `{Header, Footer}`; empty texts remove it, an empty scope
 and an unfit spec are Invalid; `Notice::Unencodable` for text and
 labels), `set_labels`' notice for a covering `<<label>>`, one
 `narrow()` reading `Order` when it narrows `All` (slice 16's hole)
 and dropping Bates entries, `join_scope` for every generator (a
 duplicate's copy unnumbered), the resolved scope an `im::OrdSet`,
 prim.rs' heap sizes. realize/key.rs: `Feed` of `HeaderFooterSpec`,
 `recipe_digest(g, page)` (spec, z, `replaces`, and the header's date,
 file and this page's number; a watermark's fed byte for byte as in
 slice 16), only the `PageCtx` fields the tokens read in the key, and
 `render_reads`' `Order` and `Labels`.
   - **Drawn and found again** (step 2). `generate::layout(g, ctx, art,
 reserved)` and `layout_digest` for both kinds: each slot that isn't
 blank its own `/Artifact /Sh… BDC q BT … ET Q EMC`, its list `<<
 /Type /Pagination /Subtype /Header (/Footer) /SheafGen (g…) >>`, in
 `Std14::from(spec.font)`, the text matrix from the shown frame
 (upright at every turn), an empty `Drawn` for a page with nothing to
 draw. `/SheafGenerators`' `header-footer` kind (the spec with its
 font, `frozen` `{date, file}`; v 1, a watermark's entry as slice 16
 wrote it) and `read()`'s checks (z `Over`, `replaces` exactly
 `{Header, Footer}`, `frozen` present, `unfit`; one a (kind, z)).
 `Marker::Gen { id, layout, recipe, number }` from `/L`, `/R` and
 `/N`; `Saved` holding the numbers and the pages whose block another
 tool merged into the content (`unnumbered`), `Saved::generator`
 rebuilding `Frozen.bates` by the new session's ids. emit/content.rs:
 `generators_on` with the header, ordered background, content,
 watermark, header; `stack` building one `PageCtx` a generator;
 `block()` writing `/N`; `needs_after` for any generator; the notes
 generalized ("No watermark, background, header or footer is drawn
 on …"). emit/catalog.rs: `/SheafHeaderFooter` written no more (kept
 in `CATALOG_OWNED`), `/SheafGenerators` for any generator. effects'
 `unsaid()` for headers (text, `first_number`, Bates spec, date, file,
 a page's number or a page left that's still there: `REMOVES |
 REPLACES_TEXT`, `RemovedKind::HeaderFooter`; font, size, colour,
 margins or more pages `CONTENT`) and `replaced()` for any
 generator's `replaces`. deps.rs' `generator()` (was `watermark()`)
 and a conservative `live(order, labels)`. realize: `base_draws` is
 `intact` for every generator, `untouched`'s `generated()` `intact()`
 alone. removal.rs: `Drawn::on` the header's slot text,
 `generators_say` (was `watermarks_say`). plan.rs' `unnumbered()`
 Note. `generate::fixture`'s legacy header (`HEADER`,
 `HEADER_SETTINGS`, `legacy_header()`, frozen bytes and catalog JSON
 as literals) and a header sheaf draws now (`SHEAF_HEADER`,
 `SHEAF_HEADER_JSON`, `SHEAF_HEADER_ID`, `sheaf_header()`).
   - **Live redraws** (step 3). `SaveRecord.drawn: Arc<DrawnBlocks>`
 (`(PageId, GenId) → DrawnBlock { layout, recipe, key }`, the key the
 removal check finds the block by), `OutputRecord.drawn`,
 `record::drawn_of(found, blob, page)` filling it at open from what
 recognition read (`Document::opened`, through `ready()` and `new()`;
 `Saved::blocks()`), a Rewrite's record the emission's map and an
 Append's the record before less the pages it wrote and the pages
 gone, plus what it drew. The emitter's `Cx.{prior, drawn, allowed}`:
 `drew()` and `keeps()` note every block written or kept, and a block
 where the saved file's is another object of the same recipe puts the
 old key in `Emission.allowed_drops` (sorted, deduplicated; none for
 an overlay); `EmitBase::Rewrite.drawn`, `Output.allowed_drops`,
 `removal::of_rewrite` passing them. effects' `LIVE_REDRAW` on an
 `Order` change under a generator reading the place, the count or the
 label, and on a `Labels` change under one reading the label, never a
 Rewrite reason. `deps::live()` writing only the pages whose model
 layout digest differs from `drawn`'s (or that have none), memoized
 by `(GenId, realize::layout_reads(needs, ctx))`. removal.rs: the
 allowed keys as a `HashSet`, and an allowed generator's old words
 left out on a page whose other blocks are judged.
 `Why::LegacyHeaderFooter` and `plan::sidecar`'s Full rule deleted;
 recovery.rs' `an_unsaved_header_s_sidecar_appends` and
 `a_live_redraw_s_sidecar_appends`; gate_7k's reorder rows
 (`planned_section`, `moved_page_drawn`).
   - **Remove by origin** (step 4). `Op::RemoveHeaderFooter { pages,
 include_authored }` (`#[serde(default, skip_serializing_if = …)]`,
 the schema additive), `ResolvedOp::RemoveHeaderFooter { pages,
 include_authored, others }`. tx/content.rs: `remove_watermark` and
 `remove_header_footer` thin wrappers over `remove_generated(lw,
 kinds, pages, include_authored)` (narrow, then each page's blocks
 with `lw.pages_done(i, n)?`, a read of `Page(p)` and `found_on`'s
 filters); `found_on` (slice 16's loop for any kinds: another file's
 blocks `DropStreams` or `DropMarked(SheafGen)`, others' artifacts a
 `Pagination` filter by the origins taken, refusals by `cut_alone`,
 what the page's filters take out already not counted again);
 `narrowed(st, kinds, pages) -> Narrowed`; `Generator::kinds()` and
 `header::kinds()`; `pub fn others_on(st, kinds, pages,
 include_authored) -> OthersFound`, which narrows a clone first.
 describe's Remove line and its run's summed counts.
 `tx::fixture::remove_header`; `others_tests` and `header_tests`
 fixtures (`AUTHORED`, `older_header_file`, `ACROBAT_HEADER_UNSAFE`,
 `RUNNING`, `UNREAD`).
   - **The app on ops** (step 5). header_footer_dialog.rs rewritten on
 the watermark dialog's pattern: `draw_preview` lowers the dialog's
 op alone; `Start::at(state, bates)` prefills from the model's (or
 the recognized) header, `Start::new(bates)` the defaults; font
 family and style chips; `HeaderFooterEvent::{Apply(Box<Op>), Remove
 { include_authored }, Dismiss}`; `HeaderFooterContext { opened,
 lower, previewer, bates, selected }`. The new generator_dialog.rs:
 the faces (moved from watermark_dialog), `scope_text`,
 `counted_pages`, `Counted::{of, line}` and `Count::{begin, take,
 said}`, the "Found" row in both dialogs. page_insert.rs:
 `set_header_footer(opened, op)` (`Validate`, "Header & footer", "No
 change to the header and footer"), the planner
 `remove_header_footer` ("Remove header & footer", "No header or
 footer to remove"), both removals through `plan_removal`,
 `line_notice`, `DocView::selected_indexes()` (Crop's rule).
 workspace.rs' `toggle_header_footer(bates)`. `sheaf_ops::
 was_cancelled` knows `EditError::Cancelled`. command_info's help.
   - **Derived sources gone** (step 6): below, Deleted; tests that used a
 page rebase as a content change ported to `ReplacePages` (stale,
 group, tx, append_rewrite tests), forms_pages' header pair on ops
 (`acrobat_headed_form`, `kept_its_fields`, `nothing_made`), crop.rs'
 `generators_keep_the_crop[_appended]`, sheaf-render's
 strip_artifacts through `PageBlocks::selected` over
 `content::splice_each`, and stamps.rs' seven strip tests ported to
 `others_tests::a_removal_takes_out_what_it_takes_and_nothing_else`
 (four pages) or confirmed covered (Checked by tests).
   - **Interop and corpus** (step 7). interop.rs'
 `a_header_s_blocks_read_in_pdfjs(_appended)` (`PDFJS_MARKED` with a
 page argument, a shared `marked_read`); `acrobat_headed_form` moved
 to tests/common; append_corpus's ignored `header_round_trips`
 (`SHEAF_HEADER_ONLY`; `appended_or_not` returns its notes);
 realize_corpus's header "Realized <<1>> of <<N>>" plus a Bates
 number (`<<n>>` was never a token, so it had drawn literally since
 slice 13).
   - **Gates, retired.rs, ledgers, docs** (step 8). gate_7k:
 `BATES_GATE` (< 50 ms on every fixture) and the Bates Rewrite's
 census of one font, asserted; `REORDER_GATE` and
 `REORDER_SECTION_MB` on the heavy, asserted (the review added the
 reorder on the file opened afresh: its section asserted too, its
 time only with `SHEAF_GATE_PROPOSAL_8`); the moved page's redraw
 asserted only with `SHEAF_GATE_PROPOSAL_8` (Deviations; the
 follow-up asserts every live row that passes on every fixture and
 lists the misses, `LiveRows`); `Census::of`
 for any generator; the module doc's slice list. retired.rs' 84 rows
 and 22 qualified-only names, each with its reason. The ledgers
 (above). docs/document-model.md's "Headers, footers and Bates as
 generators (M10.5 slice 17)" and its forward references (the
 records' sketch, blobs in memory, the open's facts, `PagOrigin`, the
 content toolkit's removal and legacy names, the ops listing, caps,
 cascades, callers, the writers lopdf keeps, sealing, emission, the
 plan's reasons, the removal check, recovery, the realizer and render
 keys, reading order, crop's flows, slice 16's hoist); README's
 Headers / footers / Bates row and its recovery paragraph.
   - **Deleted,** each confirmed dead by `cargo check --workspace
 --all-targets` and clippy, the names listed in retired.rs: all of
 stamps.rs (`HeaderFooter`, `install`, `to_content`, `to_catalog`,
 `from_catalog`, `stored`, `Stored`, `CATALOG_KEY`, `drawn_over`,
 `base_draws`, `strip`, `stripped`, `HEADER_FOOTER`, `FONT`,
 `FONT_NAME`, `MARKER`, `PageFacts`, `Placed`, `Shared`, `Bates`,
 `same_settings`; `TOKENS` and `Placed` moved to `generate::header`);
 `Source::header_footer` and its cell, `StoredHeaderFooter`,
 `open::Facts::header_footer` and model.rs' legacy header build;
 `DocState::{header_covers, bates_of, header_pages,
 number_header_pages}` and `Indexes::header`; `REPLACES_NONE`,
 `CtxNeeds::any`, `Generator`'s `z()`, `scope()` and `replaces()`
 accessors (fields now); `Cx::bates`, `emit::content::header_footer`;
 `removal::watermarks_say`, `deps::{is_watermark, Deps::watermark}`,
 `tx::content::narrow_watermarks` (renamed); `Why::LegacyHeaderFooter`
 and `plan::sidecar`'s Full rule; sheaf-ops' derive.rs
 (`DerivedSource` with `note` and `bases`, `copy_of`, `has_header`,
 `strip_stamps`, `strip_stamps_of`) and `by_source`; `save::
 derived_copy` and `copy_form`, with `graph::Loaded` (`from_pdf`),
 `copy::{inherited_attrs, INHERITED, reserve}` and `file::not_ours`
 (`graph::eager` and `copy_page_dict` stay, the Rewrite graph's and
 stamp_lib's); `model::{Derived, DerivedCopies}`,
 `Document::{derived, add_derived}`, `Snapshot.derived`,
 `SaveInput.derived`, `MemoryKind::Derived` and `Source::marked`'s
 and `DocState::backs`' derived arms; `ResolvedOp::SetPageBase`,
 `tx::Derive`, `Tx::set_page_base`, `pages::set_base`, the
 `set-page-base` name, `describe::derived`,
 `tx::fixture::commit_page_base` and `tx::Entry`'s `Base` (its `Op`
 fields the struct `Entry { op, drawn, positional }`); `Shown.content`,
 `emit::origin`, `EmitBase::{Rewrite, Saved}.derived`,
 `annots::derived` and the derived arms of pages.rs, form.rs and
 untouched.rs; the app's `header_footer_op`, `show_copies`,
 `keep_copies`, `indexes_of`, `watermark_notice`,
 `open_header_footer`, the dialog's `today` and
 `HeaderFooterContext::{current, file, scope}`. `rg -n
 'MemoryKind::Derived|derived_copy|copy_form|set_page_base|SetPageBase|strip_stamps|DerivedSource|Derive::|add_derived|DerivedCopies|stamps::'
 crates` outside retired.rs finds only `tx::forms::copy_forms` (a
 DuplicatePages helper, by its substring) and sheaf-ops' stamp
 library (`sheaf_ops::stamps`).

   Deviations from the plan (the planner's list first, as resolved):
   - **`WatermarkGen` keeps its name,** holding its content and position,
 rather than becoming §3.5's `WatermarkSpec`, which is
 `sheaf_edit::WatermarkSpec`'s. `Generator`'s z, scope and `replaces`
 moved; nothing was retired for them.
   - **`HeaderFooterSpec` takes the font** (§3.5 has none):
 `#[serde(default)]` to Helvetica, the legacy face, where
 `FontChoice`'s default is HelveticaBold. The dialog has family and
 style chips for it.
   - **`stamps::HeaderFooter` stayed `pub` until step 2 deleted it,**
 not crate-visible: `open::Facts` exposed it in a pub field that
 sheaf-ops builds with `..Default::default()`.
   - **No `impl Feed for Frozen`:** `recipe_digest(g, page)` feeds this
 page's fixed values (date, file, its number) inline; the whole map
 would cost O(n) a key. `CtxNeeds::any()` went, its one caller using
 `order()` and `label`; a Bates-only header reads neither `Order` nor
 `Labels`, its numbers being fixed. `Frozen::bates_number(page)` is
 named apart from the retired `bates_of`.
   - **A header on an empty `Pages` scope is Invalid,** as a watermark
 is, since the invariant forbids one; a legacy catalog header whose
 page indexes were all gone opened as none (step 1, before step 2
 deleted the legacy read).
   - **The page.generators cascade counts** the larger of the scope
 pages and the Bates entries gone, so for `All` it's the entries.
   - **`page_ctx` returns a `PageCtx`,** a missing label empty;
 `model_layout_digest(st, g, page, geom)` builds it, and
 `model_digest(g, &PageCtx)` serves the fuzz target, which has no
 `DocState`.
   - **`every_fact` carries a header sheaf draws today,** recognized from
 `/SheafGenerators`, since an older sheaf's catalog header opens as
 none now; the legacy fixture serves blocks_tests, fuzz's op_json
 document, others_tests and header_tests.
   - **`render_reads` reads `Order` and `Labels` whenever a generator's
 tokens need them,** intact or not: whether a page's base still
 matches depends on its place now, and the intact-only rule would
 miss a conflicting reorder.
   - **The reading order on a page with OCR and a header** is its own
 text, the header, then the recognized words, where the legacy header
 came last: the legacy OCR installer leaves `3 Tr` set without
 isolating it, so it goes after the generator blocks (putting it
 first made their text invisible; watermark.rs and watermark_tests
 caught it). sheaf-ops' text.rs search test moved its header to the
 cropped page, and the app's find test expects "page 5\nDraft\nTotal
 42". Slice 18 restores §11.4's order (a ledger row).
   - **`page_res` writes the resources inline when the OCR installer runs
 after the stack** (a bug fix): the installer copied them inline and
 left a `PageRes` placeholder naming an object never written, latent
 since slice 16 (a watermark plus OCR), found by an overlay test once
 the header joined the stack.
   - **A page whose content can't be read gets no header,** as watermarks
 don't, with the save's Note; the legacy writer installed it anyway.
 `SetHeaderFooter` refuses what `header::unfit` refuses, as
 recognition passes it over.
   - **deps' rescoped rule counts a Bates number's change** as a page's,
 so only the renumbered pages are written when nothing else changed.
   - **The header preview lowers `SetHeaderFooter` alone,** not
 `RemoveHeaderFooter` + `SetHeaderFooter` as the ledger row said: the
 header set again replaces its own old blocks, and its `replaces`
 (`{Header, Footer}` over `PagOrigin::REPLACED`) takes others' off the
 pages it covers, so the preview equals the result. The old strip
 took headers off every page, outside the scope too; Set replaces
 only on covered pages, as §3.5 says.
   - **`replaces` takes `PagOrigin::REPLACED`** (Acrobat's, an older
 sheaf's, another file's), as slice 16's watermark does, not §11.4's
 Acrobat alone.
   - **`/SheafHeaderFooter` is read by nothing:** §17 deletes
 `from_catalog`, its only reader. An older sheaf's header is told by
 the classifier (`/SheafHF`, `/SheafStamp` as `SheafLegacy`); Set
 replaces it and Remove counts and takes it, but nothing prefills
 from its settings, and `CATALOG_OWNED` keeps the key, so a save
 drops it (For Greg).
   - **`SaveRecord.drawn`'s value is `DrawnBlock { layout, recipe, key }`,**
 not §9.1's `(u128, u128)`: an `ObjKey::Block` is the `ResId` of the
 whole stream, `/SheafBlock` dictionary included, so the emitter
 can't recompute the old block's key; the key is the record map's
 `ObjKey::Block` or `ObjKey::Base { blob, obj, copy: 0 }` for the
 opened file's block. `Emission.drawn` is a flat map; an Append
 replaces the entries of the pages it wrote, read from its
 `ObjKey::Page` objects.
   - **`drawn` is filled in `Document::opened`,** from
 `facts.generators`, not `SaveRecord::opened`, which runs before
 recognition and has no `PageId`s; the test helpers fill it from the
 primary's generators. Kept blocks are noted with their own markers,
 and a kept block is an allowed drop too when the prior entry is
 another block of the recipe drawn now (a layout restored after an
 Append).
   - **Allowed drops cover a block laid out again for its boxes** (a crop
 or a turn) under the same recipe, as §3.5 lists; `LIVE_REDRAW` is set
 for `Order` and `Labels` only (§7.1), and on a step's `Order`
 classification too (information).
   - **The removal check leaves an allowed generator's old words out** on
 a page whose other blocks are judged: a restyled watermark and a
 renumbered header on one page gave a false derived veto without it.
 From the review, allowed drops are per page, since blocks are
 content-addressed and pages laid out alike share one:
 `Emission.allowed_drops` is `emit::AllowedDrops { keys, redrawn }`,
 `redrawn` the `(PageId, GenId)` pairs that drew a generator again
 from their saved block's recipe (whose old words are left out on
 that page alone), `keys` the blocks whose every page did so or is
 gone (not judged, and not read); the walk judges a shared block on
 each page that held it.
   - **The live closure memo is keyed by `(GenId, layout_reads(needs,
 ctx))`,** a digest of the geometry and only the context the tokens
 read.
   - **The reorder row moves the first half after the second:** one
 `MovePages` keeps its pages in document order, so it can't reverse
 them (a reverse was Unchanged); this renumbers every page. In
 session the "Page k of N" blocks are content-addressed and reused
 from other pages, so the section is page dicts and tree nodes.
   - **recovery.rs' encrypted Full-sidecar test** reaches Full through a
 record that can't take updates (`sound: false`), the header rule
 gone; plan_tests' `a_claim_what_sheaf_draws_breaks_gets_a_note`
 replaces the watermark's text for its Rewrite.
   - **`remove_watermark` and `remove_header_footer` stay as thin
 wrappers** over `remove_generated`; `narrow` takes kinds, a
 generator picked when `Generator::kinds()` overlaps them, and
 `header::kinds()` is the one definition of `{Header, Footer}` (Set's
 `replaces` and recognition use it).
   - **`others_on` narrows a clone first:** a block of the document's
 own header brought in from another revision of its file counts 0
 before a removal and 1 after without it (a fault run confirms the
 test catches it).
   - **What a page's filters take out already isn't counted again,** for
 Remove Watermark too: an artifact an earlier `include_authored`
 Remove filtered is cut with the rest to tell whether it's out. Pages
 without filters (every earlier test and corpus file) count and cut
 as slice 16 did; the dialogs' counts must be right after an unsaved
 Remove.
   - **Remove Watermark reports per-page progress and can be cancelled**
 (`pages_done`), on the shared lowering; a run of Remove
 Header/Footer steps sums its counts in one line, where it repeated
 the last step's.
   - **A Word-style head can't be taken by any splice** (font, colour or
 `gs` set outside a `q`): even `include_authored` leaves it, counted
 "left in place". The taken case uses heads each in their own `q`
 (`others_tests::RUNNING`).
   - **No `EditError::Content`:** no op of this slice fails on content
 (Set reads none, Remove counts an unreadable page), so slice 18 has
 it, as slice 16 left it.
   - **The dialogs' shared code is a module of its own,**
 `generator_dialog` (the faces moved, so no rows); the count runs
 `others_on` twice in one background task, without and with the
 author's own, so the line can say how many of the author's Remove
 can take; once a dialog opens, never a keystroke; checked against
 the opened snapshot by `(Snapshot.instance, version)` and unit-tested
 through `Count::take` (no headless test builds a dialog window). "The
 selection" is Crop's rule (more than one page selected), now
 `DocView::selected_indexes()`; Remove still removes from every page.
   - **Header & Footer… and Bates Numbering… toggle one dialog slot,** as
 `toggle_watermark` toggles the two z's; opening it doesn't close a
 Watermark dialog, as slice 16's didn't. Its Remove button is always
 shown. The dialog builds the op itself, so `header_footer_op` went;
 empty texts can't be applied ("Type something in at least one
 place"). Notices are each commit's line ("Set the header and
 footer"; the "shown now, written when you save" suffix went).
   - **`sheaf_ops::was_cancelled` knows `EditError::Cancelled`,** which
 the shared lowering's `pages_done` returns: a planner stopped part
 way said "… failed: canceled".
   - **Content-change tests use `ReplacePages`,** not `SetOcrText` or a
 filter-adding Remove as planned: those don't change a page's base,
 and the tests are about bases (two pages taking one copy, a stale
 base, a revert conflict, a page shown as another).
 `append_rewrite_a_page_replaced_by_its_own_page_is_a_form_of_its_own`
 asserts copy 2 (ReplacePages brings the form as an instance of its
 own); the two "rebased" tests are "replaced", the second's Rewrite
 now from OtherTarget plus Removes.
   - **`crop.rs`' corner tolerance is 3 pt:** PDFium's glyph box for
 "HEAD" tops out 2.25 pt above the cap height; a layout on the
 uncropped box would be 50–72 pt off.
   - **`graph::load` didn't exist;** `graph::eager` and `copy_page_dict`
 stay, the Rewrite graph's and stamp_lib's. rewrite_tests'
 `read_pages` walks `/Parent` itself, and the open test calls
 `file::is_ours`.
   - **The header tier and interop save a reorder as planned** and
 assert an Append (a live redraw is one by design); the tier appends
 a reorder whose signatures ask anyway (counted; the removal check
 still stops on any finding then), and checks beyond the bullet:
 `LIVE_REDRAW` in the path flags, the author's own count unchanged,
 Bates numbers moving with their pages, none of others' left, and 0
 derived vetoes.
   - **Proposal 8's misses aren't asserted:** the redraw misses after a
 real attempt (Found), and so does the review's reorder on the file
 opened afresh (The review). Greg kept the page numbers live and
 accepted the misses (2026-10-22; the follow-up below), so `gate_7k`
 asserts every live row that passes and those that miss
 (`ACCEPTED_MISSES`) only when `SHEAF_GATE_PROPOSAL_8` is set (every
 gate test is ignored already, so a test of its own would fail the
 gates' run); each such row says "accepted miss (Greg, 2026-10-22);
 option A, a lead window, to revisit". A transitional ledger row says
 so.
   - **retired.rs:** no module row for `stamps` (sheaf-ops' stamp
 library is `stamps` too), and none for names that moved
 (`Generator`'s fields, `TOKENS`, `Placed` → `Line`,
 `GenSpec::same_place` → `Generator::same_place`, the faces) or were
 made and deleted within the slice (`HeaderFooterSet`). The
 `set-page-base` string has no row: retired.rs matches paths.

   Found while building:
   - **Proposal 8's redraw misses on the heavy, and the reorder isn't
 why** (step 8's probe): a page drawn in a group none drew before
 costs 3.7 ms for the view (the group's 128 members and keys), 15.5
 ms realizing the group's 128 deltas (each page 87 µs: 60 µs of it
 the header's content, its page classified for `replaces`, about half
 of that cold, then laid out, digested and compressed; 14 µs
 annotations; 14 µs the root and catalog every delta emits and
 `Part::page` drops) and about 15 ms for PDFium's open and cold
 render. Any header on every page costs that for a
 group not drawn before, frozen numbers too (32 ms with Bates alone);
 what live tokens add is that a reorder makes every visible group new
 (39 ms for a page drawn before the move, where Bates alone redrew it
 from its engine in 10.5 ms). A prototype emitting a group's deltas
 on 16 threads (scratch only, restored and checked with `cmp`) drew a
 moved page in 28 ms: not enough, and a threading change to the
 realizer besides.
   - **Bates apply cost nothing but its classify:** one `SetHeaderFooter`
 on a fresh open is 0.62–0.64 ms (slice 16: 157, 211 and 85 ms
 through `strip_stamps`).
   - **realize_corpus's header drew `<<n>>` literally** since slice 13
 (step 7): not a token.
   - **`append::Refused::Held` has no test now:** its only one was the
 derived-copy pair. A probe (the first page deleted and brought in
 again, with and without an `/OpenAction` to it) gets copy 0, no
 refusal, an Append, and a reopen as a fresh Rewrite gives; `held()`
 stays as a guard.
   - **An approval signature still makes a reorder ask** (step 7's tier,
 pre-existing): `forms::sigs::sig_impact` judges by `/V` and
 permissions, not by whether the bytes verify, so a file force-
 rewritten with `break_signatures` still asks; 25 of the 62-file
 sample's 100 reorder Appends.
   - **The corpus checkout's new layout** (slice 16's Found) still makes
 every corpus test skip against `../pdf-corpus`; every run here went
 through the scratch root of links.

   The review (13 findings on the slice's changes, each checked against
   the code; all fixed):
   - **Proposal 8's gate measured the best case:** in the session that
 saved the header, the blocks a reorder draws are numbered in the
 file already (another page's before), so the section was page
 dictionaries and the tree. gate_7k now also opens the work copy
 afresh, moves the same half and saves: every block is written anew
 (3.33 MB on the heavy, within 8 MB, asserted) in 1.43 s (gate run;
 1.65–1.95 s in a probe), past 1 s. The probe's breakdown on the
 heavy: deps 60 ms; the emission about 1.1 s, of it 0.31 s
 classifying each page's content cold for others' headers to replace
 (`replacing`, `Source::blocks`) and 0.53 s rebuilding each page's
 content around its new block (`rebuilt`, its streams read and
 counted); the removal check 0.36 s (0.39 s before the change below);
 the rest numbering, the read-back and writing. Tried: the check
 skips an allowed block before reading it (safe: such a block was
 never judged); not tried, as each needs its own proof of identical
 output: classifying the written pages in parallel ahead of the
 emission, and a fast path for a page whose blocks alone change.
 The time is asserted only with `SHEAF_GATE_PROPOSAL_8`: Greg
 accepted the miss and kept live tokens (2026-10-22; the follow-up
 below).
   - **Allowed drops were per key, not per page** (two findings, one
 fix): a header's block is content-addressed, so a count-only header
 is one object on every page it covers, and one page's live redraw
 let it go everywhere, the words of a page that lost the header in
 the same save never judged. Now per page (Deviations); test:
 `a_shared_block_drawn_again_elsewhere_is_judged_where_it_went` (a
 page added where the header isn't, the header narrowed off one of
 five, misjudged: a derived veto and a Rewrite).
   - **A block was kept for its layout alone:** `intact` and `rebuilt`'s
 kept block compared `/L` but not `/R`, so a change that lays out
 alike (a centered slot's side margins, Bates numbers no text draws)
 kept blocks whose recipe and number were stale, and the record and a
 reopen read them: a false derived veto on the next reorder, and old
 Bates numbers back after a renumber. Both now require the recipe
 drawn there now (`recipe_digest`, the page's number in it); the
 realizer asks `intact` too, so such a page draws itself. Tests:
 `a_change_laid_out_alike_draws_its_blocks_again` and
 `bates_numbers_not_drawn_are_saved_as_renumbered`.
   - **Bates numbers could run past `u64::MAX`** (a panic in debug, 0
 after the largest in release): `header::numbered` is checked and
 returns None, and `set_header_footer` refuses it (Invalid). Test:
 `bates_numbers_never_run_past_the_largest`.
   - **The merged-header save note** searched `Saved::unnumbered`
 linearly for each page (O(pages²) for a file another tool merged
 every page of): it's in page order, so a binary search.
   - **Dead and stale code:** `header::draws` (its test reads `slots`),
 header.rs' own `CAP_HEIGHT` (its parent's, documented as one
 approximation for every Std14 face), `ResolvedOp::name`'s doc naming
 `set-page-base`, and retired.rs' qualified-only `rebase` (the tests'
 helper is `replaced` now; `page_insert::DocView::rebase` is checked
 bare again).
   - **The fuzz target and dictionaries** say headers: the target's doc,
 pdf.dict's `"header-footer"` and `"frozen":` (`/N` was there),
 op.dict's `"font":`, `"bates":`, `<<label>>`, `<<bates>>` and
 `<<N>>`.
   - **Comments re-wrapped** where the slice left them ragged or long
 (overlay_tests, realize/key.rs, emit/content.rs, source/fuzz.rs,
 gate_7k).
   - **The count dialogs' "page N" line** couldn't be reached
 (`selected_indexes` is empty unless more than one page is selected):
 the arm is gone, its test counts two pages.

   Proposal 8 kept live (Greg, 2026-10-22: page numbers stay live, the
   renderer made faster where it safely can, and what still misses
   accepted, a lead window to revisit; not frozen numbers plus Update):
   - **C1, a header laid out once a page:** `DocState::laid` (`Laid`, in
 `Indexes`, one for every version derived from an open) keeps a
 header's layout digest by `(GenId, recipe_digest(g, page),
 layout_reads(needs, ctx))`, all a header's layout reads
 (`emit::content::header_digest`, `laid_key`). `untouched`,
 `render_key` (through `model_layout_digest`), deps' live closure
 (`laid_digest`), and the emitter's `intact` and `rebuilt`
 (`digest_of`) ask it, and `drawn` adds the digest of each block it
 lays out. Watermarks stay out: the emitter reads their art from the
 file. Past 32,768 digests it starts again. Debug builds lay out
 again every digest it returns and assert they agree.
   - **C2, deltas without the frame:** `EmitScope::Delta(page)` is an
 overlay's page with no root, catalog or form, which `Part::page`
 dropped from every delta and the frame writes; the realizer's
 deltas use it. A page's `/Parent` is the root's placeholder still,
 and the catalog's number is claimed as before.
   - **What they bought** (the first draw's split, temporary
 instrumentation in `Realizer::view`/`open` and the pool's open and
 render, restored and checked with `cmp`; a scratch test drawing
 each case's page in 5 groups none drew, medians in ms; "find" is
 the group's members and keys, with the layouts digested there and
 in the deltas):

 | Fixture, case | find (digests) | deltas (digests) | frame + assemble | PDFium open | render | total, before → after |
 |---|---|---|---|---|---|---|
 | synthetic, set, unsaved | 0.2 (0) → 0.3 (0) | 12.2 (0) → 12.1 (0) | 1.0 → 0.9 | 1.8 → 1.8 | 7.2 → 7.3 | 22.7 → 22.7 |
 | synthetic, saved, all moved | 0.5 (0) → 0.5 (0) | 9.6 (0) → 9.4 (0) | 1.1 → 1.0 | 1.7 → 1.7 | 7.0 → 6.9 | 20.2 → 19.8 |
 | synthetic, saved, one deleted | 0.2 (0) → 0.4 (0) | 8.9 (0) → 8.8 (0) | 1.0 → 1.0 | 1.8 → 1.8 | 7.2 → 7.1 | 19.1 → 19.3 |
 | synthetic, opened again, all moved | 1.8 (256) → 2.0 (127) | 16.5 (128) → 16.5 (0) | 1.0 → 1.0 | 7.0 → 7.1 | 8.2 → 8.0 | 34.4 → 34.9 |
 | synthetic, opened again, one deleted | 4.4 (256) → 3.0 (127) | 18.1 (128) → 16.6 (0) | 1.1 → 1.0 | 7.0 → 7.0 | 7.9 → 7.5 | 38.8 → 35.4 |
 | heavy, set, unsaved | 0.9 (0) → 0.9 (0) | 17.1 (0) → 15.3 (0) | 2.0 → 1.7 | 7.9 → 7.8 | 11.6 → 11.3 | 39.7 → 37.5 |
 | heavy, saved, all moved | 3.9 (0) → 6.2 (0) | 14.6 (0) → 14.0 (0) | 2.1 → 3.2 | 9.0 → 12.1 | 9.1 → 10.6 | 39.0 → 46.4 |
 | heavy, saved, one deleted | 4.5 (0) → 4.6 (0) | 13.7 (0) → 12.3 (0) | 1.8 → 1.9 | 7.9 → 7.8 | 11.7 → 11.1 | 39.8 → 38.3 |
 | heavy, opened again, all moved | 3.7 (256) → 2.0 (127) | 20.5 (128) → 20.3 (0) | 1.8 → 2.0 | 10.4 → 10.4 | 9.3 → 9.3 | 46.3 → 44.3 |
 | heavy, opened again, one deleted | 5.0 (256) → 4.2 (127) | 21.7 (128) → 18.6 (0) | 1.9 → 2.0 | 10.1 → 10.4 | 8.7 → 9.3 | 47.9 → 44.7 |
 | scanned, set, unsaved | 0.2 (0) → 0.3 (0) | 11.1 (0) → 11.0 (0) | 1.7 → 1.7 | 3.1 → 3.2 | 23.4 → 23.3 | 42.2 → 42.3 |
 | scanned, saved, all moved | 0.5 (0) → 0.5 (0) | 10.0 (0) → 10.3 (0) | 1.7 → 1.9 | 2.7 → 2.9 | 17.8 → 18.2 | 33.0 → 34.5 |
 | scanned, saved, one deleted | 0.4 (0) → 0.4 (0) | 9.7 (0) → 8.8 (0) | 1.3 → 1.2 | 2.8 → 2.7 | 18.8 → 18.2 | 33.2 → 31.5 |
 | scanned, opened again, all moved | 2.2 (256) → 1.2 (127) | 13.6 (128) → 12.6 (0) | 1.2 → 1.2 | 9.1 → 9.2 | 19.1 → 19.5 | 45.2 → 44.0 |
 | scanned, opened again, one deleted | 3.7 (256) → 2.7 (127) | 13.9 (128) → 12.4 (0) | 1.2 → 1.1 | 9.1 → 9.1 | 17.6 → 17.6 | 45.6 → 42.9 |

 Little: the "four layouts a page" the scoping counted were digests
 that run only where the base page holds the header's block (the
 file opened again); in the session that set or saved the header
 the base pages hold none, and each delta lays the page out once, as
 it must (8 µs a page, 1 ms a group). C1 halves find's digests and
 takes the deltas' away (1–4 ms on a file opened again); leaving
 out C2's root, catalog and form saves 0.2–1.8 ms a group, within
 the noise of one run (step 8 estimated 14 µs a delta, 1.8 ms a
 group). What a group costs is
 the deltas' emission itself (70–160 µs a page: the page's
 dictionary, its content stack and isolation, its resources,
 annotations), PDFium's open and its cold render, each paid for all
 128 pages but the render. Only drawing fewer pages first (option A)
 moves it. An A/B of the reorder's Appends, C1 off and on, three
 rounds each in turn: no change in the session (the heavy's 0.48–0.87
 s either way) and 0–0.1 s less on a file opened again.
   - **The everyday triggers** are gate rows now (`LiveRow`), on every
 fixture, each draw a page at 1024 px in a group none drew before
 (`first_drawn`): "Page n of N" just set on every page, unsaved;
 every page moved after a save (the moved page, as before); one page
 deleted after a save, in the session and on the file opened again;
 and the reorder's Append in the session and on the file opened
 again. Rows that pass are asserted on every fixture (the Appends ≤
 1 s, the sections ≤ 8 MB, the draws within the fixture's realize
 gate; the scanned has none); those that miss are `ACCEPTED_MISSES`,
 asserted only with `SHEAF_GATE_PROPOSAL_8`, each row saying
 "accepted miss (Greg, 2026-10-22); option A, a lead window, to
 revisit". No assert was loosened: the Append and section gates
 moved from the heavy alone to every fixture.
   - **Tests:** `a_header_s_layouts_are_kept_as_laid_out_in_every_version`
 (key_tests: every page's kept digest is a fresh layout's through a
 move, a delete, a relabel, a turn, a respecified header and every
 undo, and all versions share one memo);
 `the_predicate_agrees_with_the_emission_in_every_case` now also
 checks each page's delta (`delta_alike`): what it writes that the
 page reaches is byte for byte what the whole overlay of the page
 wrote, at the same numbers, and it writes nothing the overlay
 didn't, in every case and under every input (`Part::reached`).
   - **Fault checks** (restored from my own copies, checked with `cmp`;
 no proptest seed added): the memo's key without the layout's reads,
 and without the recipe (the memo test fails, by the debug
 assertion); the delta without the page's claimed numbers (the
 predicate test fails on its first page).

   Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
   - **The gates** (`gate_7k`, release, each test in its own run; the
 follow-up's final run on the code as committed, load 1.9 to 3.9,
 11 of 11 pass; the slice's own run before the follow-up in
 brackets where a row is new or moved):

 | Row | Synthetic | Heavy | Scanned |
 |---|---|---|---|
 | Bates apply, all pages, one op on a fresh open (asserted < 50 ms) | 0.617 ms (slice 16: 157) | 0.637 ms (211) | 0.632 ms (85) |
 | the Bates Rewrite, and the fonts it adds (asserted 1) | 419 ms, 1 | 949 ms, 1 | 921 ms, 1 |
 | "Page n of N" set on every page, unsaved: a page drawn 1,024 px, a group none drew (the realize gate 15 and 25 ms; accepted misses) | 22.3 ms | 36.7 ms | 33.6 ms |
 | saved, every page moved: Append planned and written (asserted ≤ 1 s), RSS added, section (asserted ≤ 8 MB) | 469 ms, 66.5 MB, 1.36 MB | 655 ms, 89.3 MB, 1.47 MB [624 ms] | 469 ms, 77.9 MB, 1.37 MB |
 | saved, every page moved: a moved page drawn (accepted misses) | 18.8 ms [20.3] | 33.9 ms [35.4] | 32.4 ms [32.2] |
 | saved, one page deleted: a page drawn (accepted misses) | 19.3 ms | 31.8 ms | 31.0 ms |
 | opened again, every page moved: Append (asserted ≤ 1 s on the scanned; accepted misses on the others), RSS added, section (asserted ≤ 8 MB) | 887 ms [1,009; 1,068 in the follow-up's first run], 108 MB, 3.31 MB | 1,569 ms [1,433], 159 MB, 3.33 MB | 938 ms [1,022], 113 MB, 3.32 MB |
 | opened again, one page deleted: a page drawn (accepted misses) | 27.3 ms | 41.8 ms | 41.0 ms |
 | thumbnail strip, crop + watermark + "Page n of N" header, 320 px (asserted ≥ 310 pages/s, ≤ 96 MB weighed and added) | 420 pages/s, 80.7 MB, 25.7 MB | 346 pages/s, 80.7 MB, 65.4 MB | 163 pages/s, 80.8 MB, 82.1 MB |
 | realize + open + render 1,024 px, an edited page (asserted < 15 and < 25 ms) | 12.0 ms | 21.2 ms | 31.8 ms |
 | crop all pages, and validated after a delete (asserted < 200 ms) | 13.3, 15.2 ms | 11.1, 14.7 ms | 11.1, 14.5 ms |
 | census of a watermark and a PNG background (asserted 1, 1, 1, 0; page resources) | 1, 1, 1, 0; 1 of 1 | 1, 1, 1, 0; 70 of 70 | 1, 1, 1, 0; 7,000 of 7,000 |
 | engines after 220 groups: weighed, RSS added, open (asserted ≤ 96 MB, ≤ 96 MB, ≤ 6) | 28.9 MB, 41.5 MB, 6 | 28.9 MB, 65.3 MB, 6 | 29.0 MB, 61.8 MB, 6 |
 | one-mark Append on the scanned (asserted < 50 ms) | | | 30.3 ms |
 | snapshot, `Tx::check` of a delete of every page (asserted < 50 µs, < 1 ms) | 0.241 µs, 0.388 ms | 0.280 µs, 0.389 ms | 0.241 µs, 0.390 ms |

 After the review's fixes (load 2.2 to 3.9), every asserted gate
 holds: the reorder in session 477, 654 and 600 ms (1.36, 1.47 and
 1.37 MB); realize + open + render 11.9 and 21.3 ms; the strip 419
 and 351 pages/s; crop 12.9, 10.2 and 13.3 ms; Bates apply 0.611,
 0.645 and 0.648 ms; the in-process Rewrite against slice 8's 1.14×
 and 1.08×; the one-mark Append 20.5 ms.

 Step 2's checkpoint (the header drawn as blocks, its effects, the
 legacy writer gone): realize + open + render 12.0 and
 21.6 ms, the strip 420 and 349 pages/s, the Rewrite 1.12× and 1.11×
 of slice 8's, Bates apply still 141, 195 and 77 ms through
 `strip_stamps`. Step 3's early read of proposal 8 on the heavy: 695
 ms, 1.47 MB and 38.4 ms. The rest as slice 16 left them: the
 geometry walk 47.8 and 77.4 ms (< 100 and < 250 ms); ready on the
 heavy 376 ms (< 1 s); 200 OCR versions of 1,000 pages at most 236
 MB (≤ 256); AddMark 0.019 ms and a full reorder 0.963 ms at 20,000
 marks; 1,000 mark adds on the heavy 0.033 ms (blank 0.022 ms);
 ready on 20,000 sheaf marks 521 ms (< 1 s); their Rewrite 1,503 ms;
 Trim White 41.6, 42.8 and 100.8 s (no gate). The follow-up's two
 runs, the rest: the geometry walk 48.5 and 76.4–79.4 ms; ready on the
 heavy 346–350 ms; 200 OCR versions of 1,000 pages at most 236 MB;
 AddMark 0.020 ms and a full reorder 0.947–1.35 ms at 20,000 marks
 (< 2 ms); 1,000 mark adds on the heavy 0.033 ms (blank 0.023 ms);
 ready on 20,000 sheaf marks 536 ms; their Rewrite 1,321 ms; Trim
 White 38.7, 42.6 and 105.5 s (no gate).
   - **The Rewrite against slice 8's writer**
 (`rewrites_on_the_synthetic_and_heavy_7k` five times, each fixture
 in a process of its own, against slice 8's 134 and 707 ms):
 synthetic 148–155 ms, median 150 ms, 1.12× (1.10–1.15×); heavy
 760–817 ms, median 776 ms, 1.10× (1.08–1.16×); no run past the 1.2×
 flag (slice 16: 1.15× and 1.14×, one synthetic run past it). A
 document with no generator takes the same calls (`generators_on`'s
 early out). The follow-up's two gate runs (the gates' Rewrite part
 and the Rewrite test, each in a process of its own): synthetic 145,
 146, 152 and 153 ms (1.08–1.14×), heavy 761, 778, 782 and 791 ms
 (1.08–1.12×); none past the flag.
   - **The corpus** (through the scratch root), every count as slice 16
 left it: materialize_corpus's identity 4,389 of 4,392 written,
 2,269,722 primary objects surviving, 919,479 copied as they were,
 436 changed by `keep_only`, 0 failed; 2,901 forms without their
 first page, 0 failed; every page duplicated, 4,389, 0 failed;
 inserted twice into the 1040, 1,098, 0 failed; four times, 0
 differ; the logo, 0 of 2,037,600 bytes differ. append_corpus:
 4,389 marked, 3,770 appended, 259 rewritten (211 RemovalCheck, 34
 BaseObjectsMissing, 14 BaseUnsound), 360 asked, 0 failed, 0 derived
 vetoes and 211 on the files' own objects; the oracle finds text gone
 from 2,899 of 2,901 (none from 0016064 and 0112654).
 encrypted_corpus: the Rewrite keeps 573 of 573 and the Append 572
 (0630899 declined). The header tier (step 7, `SHEAF_HEADER_ONLY`)
 on its six files (0025448, 0187215 signed and PDF/A, 0861328, 0442193
 Word's, the 1040, 0630899) and on 62: 0 failed, none of others'
 headers left, 0 derived vetoes. The full ignored tiers (deleted
 means deleted, realize_corpus's predicate and pixel tiers, the
 watermark and header tiers) are the pre-merge run's (Greg,
 2026-10-20).
   - **Suites:** `cargo test --workspace` 1,544 passed and 45 ignored
 (slice 16: 1,491 and 42; ignored +3, the interop pair and the header
 tier; the review's four tests and the follow-up's memo test in the
 1,544). After the follow-up, the corpus counts as above, and
 realize_corpus's predicate and pixel tiers on six files
 (`SHEAF_REALIZE_ONLY`: the 1040, 0025448, 0187215, 0861328, 0442193
 and 0630899, which is realized standalone): the model disagrees on 0
 of 180 pages and inputs, and 0 of 30 pages differ. After the review the
 corpus counts are as above, the header and watermark tiers pass on
 their six files with 0 derived vetoes, the interop tier 13 of 13 and
 both release properties pass. fmt and clippy (`-D warnings`, all targets) clean, and clippy
 with sheaf-app's Linux-only gates switched off
 (`nonlinux-clippy.sh`); clippy (`-D warnings`) on
 `x86_64-pc-windows-msvc` passes for sheaf-doc, sheaf-edit,
 sheaf-util and sheaf-image with their tests and for sheaf-render,
 sheaf-print, sheaf-ops and sheaf-ocr with all targets. The release
 properties: Append ≡ Rewrite at 1,024 cases and the replay property
 at 4,096 pass (65 s together). The interop tier (qpdf; pdf.js
 6.4.299 on node 24.22): 13 pass with `--include-ignored` (slice 16:
 11), `qpdf --check` clean on every output; pdf.js reads a header's
 page as its own text, then each slot once in its own `Artifact`, and
 after the trade page 1 reads "Page 2", "Page 1 of 2" and SHF0002.
   - **The reviews:** the plan was reviewed adversarially before the
 steps started, and its findings are the deviations above, as
 resolved. Steps checked their new checks by switching their
 behavior off (below).

   Checked by tests:
   - **The model** (tx/header_tests.rs, 12): Bates fixed by id across a
 move; identical settings make no version (id, date, file and map
 equal); `All` covering a page inserted later, unnumbered, and a
 duplicate's copy unnumbered; a delete dropping its number, never
 reused; an extract's `subset` keeping the kept pages'; Remove
 narrowing or deleting; a stale Remove before an insert conflicting
 on `Order` (Validate) or lowered onto the inserted page (Relower);
 the invariants; the label notice; a cancelled Remove leaving the Tx
 as it was; `others_on` equal to both ops' counts with and without
 `include_authored`; Bates numbers refused past `u64::MAX`.
 generate/header.rs' 5; key_tests (a reorder
 changes no Bates-only key; renumbering changes those pages'; `<<1>>`
 and `<<label>>` keys move with place, `<<N>>`'s don't, labels change
 only `<<label>>` keys; `render_reads`); sheaf-edit's
 `a_header_reads_in_helvetica_unless_it_says`; effects'
 `headers_and_footers`, `live_tokens_are_drawn_again` and
 `a_header_replacing_others_takes_them_off`.
   - **Drawn, found, redrawn** (materialize/header_tests.rs, 27, eight
 cases both ways): the block's shape and `/N`; `All` reaching pages
 inserted before and after a reopen; Bates surviving a save, a reopen
 and a reorder; a merged block's Note; an extract's binder numbers
 and "Page 1 of 2"; `?` for an unencodable label, both ways; Acrobat's
 replaced and Word's kept; 1.3 → 1.4; found again, written as it is,
 updated in place, removed with no trace; what sheaf wouldn't write
 not found; an older sheaf's replaced, its settings dropped; a
 reorder after a save appending with `LIVE_REDRAW`, the closure {the
 two pages, `PagesNode(0)`}, at most 2 blocks, no vetoes, `drawn`
 after a Rewrite, an Append and an open; a text change a Rewrite with
 the old words gone, margins and numbers appending; a misjudged block
 still stopping an Append; a label change appending; a shared block
 judged on the page it went from; a change laid out alike drawing
 its blocks again, a reorder after appending; Bates numbers no text
 draws read back as renumbered. removal_tests'
 `an_allowed_live_redraw_s_old_block_may_go` (a block allowed, or its
 page drawing it again); recovery.rs' unsaved
 header and live-redraw sidecars; append_rewrite_tests' header over a
 reorder appending and the property's live and Bates headers;
 generate tests (WinAnsi, identical bytes, four turns, nothing to
 draw); untouched_tests' header cases.
   - **Remove by origin** (materialize/others_tests.rs, 11 more, 24 in
 all, seven cases both ways): Acrobat's taken a page at a time (summed line, REMOVES,
 a filter a page, the form and text gone, the count 0 after); the
 author's left, counted, and with `include_authored` the one in its
 own `q` taken; an older sheaf's; another file's block by
 `DropStreams`; a revision's block once the header's off; a refused
 Acrobat header's Note and counts, an unreadable page; and the four
 ported strip cases in
 `a_removal_takes_out_what_it_takes_and_nothing_else`. stamps.rs'
 seven: the legacy strip, kinds alone, an inline image holding `EI`
 and a header across two streams ported there (pages 1–4); a shared
 stream covered by `a_stream_blank_pages_share_stays_where_uncovered`;
 unreadable content and an unsafe cut by `a_header_a_splice_refuses_
 stays_and_is_counted`, with blocks_tests' and content tests' own.
   - **The app** (Linux headless and unit): `a_header_is_one_undo_step`
 and `remove_header_footer_is_planned_and_counts_the_author_s`; the
 preview equal to applying; the start state before and after a save
 and reopen; the count (Acrobat's, an author's in its `q`, a
 Word-style head; selection pages; another version's or document's
 count dropped; "Counting…"); the two content-change docview tests on
 `SetOcrText`; tests/progress.rs' stopped planner.
   - **No derived sources:** the rg above; retired.rs' rows; forms_pages'
 header pair both ways in `own_cache` (no source but the primary,
 fields on screen, the form check clean, 4 fields and widgets,
 values, `/P`, appearances, Acrobat's off every page, the footer
 there or gone); crop.rs' `generators_keep_the_crop[_appended]`.
   - **Interop and corpus:** `a_header_s_blocks_read_in_pdfjs(_appended)`;
 the header tier.
   - **Fault checks** (each restored from its step's own copy and
 checked with `cmp`; no proptest seed added): step 3's allowed drops
 off (removal_tests' allowed case gives a derived veto); step 4's
 `others_on` without its narrowing (the revision-block test fails);
 step 6's forms_pages expecting Acrobat's header after Remove (fails
 both ways); step 7's header tier over a fault in the emitter's
 content stack (it fails); step 8's retired.rs rows, each of the 84 hit by a
 probe file defining it at its path and naming it, and the 38 rows
 ending in a qualified-only name not hit when bare (a package cargo
 doesn't build, deleted). The review's: `intact` without the recipe
 (the laid-out-alike and Bates tests fail), `rebuilt`'s kept block
 without it (the same two), `AllowedDrops::of` keeping every
 candidate and the walk judging a shared block on its first page
 alone (the shared-block test fails each time); no proptest seed
 added.

   For Greg:
   - **Proposal 8: page numbers live, the misses accepted** (your ruling,
 2026-10-22). After the follow-up's speedups (C1 and C2, both safe,
 both small; Proposal 8 kept live, above) the misses are the first page
 drawn of a group every page of which is laid out anew (set unsaved,
 every page moved, one deleted, and one deleted on the file opened
 again): 22.3, 18.8, 19.3 and 27.3 ms on the synthetic (gate 15), 36.7,
 33.9, 31.8 and 41.8 ms on the heavy (gate 25); and the reorder's
 Append on a file opened again: 1.57 s on the heavy and 0.89–1.07 s on
 the synthetic (two gate runs; 1.06–1.11 s in three A/B rounds),
 against 1 s. Everything else passes and is asserted: the Append in the
 session that saved the header (469, 655 and 469 ms), every section (at
 most 3.33 MB against 8), and the scanned's Append opened again
 (0.89–0.94 s, near its gate). A group costs its 128 deltas' emission,
 PDFium's open and its cold render, so only drawing fewer pages first
 moves it: option A, a lead window, to revisit. The Append opened again
 needs the emission's per-page work in parallel or a fast path for
 pages whose blocks alone change (Left for later).
   - **An older sheaf's header isn't prefilled:** `/SheafHeaderFooter`
 is read by nothing; its blocks are `SheafLegacy` by `/SheafHF` or
 `/SheafStamp`, replaced by Set and counted and taken by Remove, and
 a save drops the catalog key (`CATALOG_OWNED`). Slice 19's
 RemovePrivateData would rename what's left in old files.
   - **The preview lowers `SetHeaderFooter` alone** (Deviations), and
 Set replaces others' headers only on the pages it covers, where the
 old strip took them off every page.
   - **`SaveRecord.drawn` holds each block's key** beside §9.1's two
 digests (Deviations).
   - **Wording, yours:** the count line ("On all pages: 1 made in Acrobat
 will be replaced; 2 the author's stay (Remove can take 1 of them)",
 "Counting…", "On all pages: no header or footer others drew",
 "…, as taking them out would change the page", "any inside forms on 2
 pages stay", "3 pages can't be read"); Remove's line ("Remove the
 header and footer from pages 1–3 (2 made in Acrobat; 1 the author's
 left in place)"); notices "Set the header and footer", "No change to
 the header and footer", "No header or footer to remove"; undo labels
 "Header & footer", "Remove header & footer"; the save Notes "No
 watermark, background, header or footer is drawn on page 3, whose
 content can't be read." and "The Bates numbers on pages 2, 3 can't be
 read: another program merged the header into the page. They're saved
 without them; set the header and footer again to number them."
   - **`include_authored` in the app** is a chip, "☐ the author's own
 too", on a "Remove" row of the dialog, as the watermark's; a Word-
 style head (font or colour set outside a `q`) can't be taken even
 then, and the line counts it "left in place".
   - **The counts** are of the selection only when more than one page is
 selected (Crop's rule), else every page; Remove still removes from
 every page. They leave out what an earlier unsaved Remove filtered.
   - **No field for the date or file name:** both are fixed when the
 header is first applied, and an update keeps them; setting it again
 after a Remove takes today's.
   - **Until slice 18 a page with OCR and a header reads** its text, the
 header, then the recognized words (Deviations).
   - **A page whose content can't be read gets no header,** as with
 watermarks, said in a Note.
   - **`append::Refused::Held`** has no test now (Found).
   - **An approval signature asks on a reorder** even after
 `break_signatures` rewrote the file (Found; not this slice's).
   - **The manual Acrobat round trip** (Asks) for the header as for the
 watermark: open a file sheaf saved with a header in Acrobat, edit
 Acrobat's own header there, save, reopen in sheaf.

   Left for later slices:
   - **Slice 18:** `EditError::Content` for a `ContentError` (no op here
 fails on content); the `/SheafOcr` given name and the
 `text_layer` installers (`install`, `to_content`, `MARKER`,
 `FONT_NAME`, kept qualified-only in retired.rs); the OCR layer a
 block of its own before the generators drawn over the page (§11.4),
 which restores the reading order and lets `page_res`' inline case
 go.
   - **Slice 19:** RemovePrivateData of `/SheafHeaderFooter` (dropped
 from catalogs sheaf writes already), `/SheafHF` and `/SheafStamp`
 in old files.
   - **A later perf pass:** the strip's per-page header layout, digest
 and `PageRes` in overlays; a reorder's Append on a file opened
 afresh (each page's classification and rebuild, the removal check),
 live tokens being kept; the group realization proposal 8's redraw
 waits on (a delta's emission, 70–160 µs a page; the view's 128
 render keys, 0.3–6 ms a group; deltas in parallel), beside option A;
 slice 16's items. A delta's root and catalog, and a header laid out
 four times, went in the follow-up (C2, C1).
   - **Option A, a lead window** (Greg, to revisit): a group's visible
 pages realized ahead of it, which the accepted misses wait on; until
 then `SHEAF_GATE_PROPOSAL_8` asserts them (`ACCEPTED_MISSES`, a
 ledger row).
   - **The corpus layout:** `Corpus::find` and realize_corpus's inventory
 name taught `../pdf-corpus`'s new layout (another session's change).
   - **Follow-ups:** the `Closure::stale` probe (slice 11's Unprobed),
 untouched: no step changed that walk.
