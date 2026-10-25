# M10.5 slice 11: Ops and Tx

Status: done · Part of [M10.5: the edit API](README.md)

**Ops and Tx** (XL; proposals 20 and 25; bugs 8, 11, 12, 25, 27 and
29).
- **The `sheaf-edit` crate:** `Op`, `Transform`, the per-kind refs,
  `PageSel`, `Patch`, `NewMark`/`MarkPatch`, `Imported`, `BlobSel`,
  the specs, the envelope, schemars, `deny_unknown_fields`, and a
  custom ref deserializer.
- **The types ops carry move into it** (§6.1): `PageId`,
  `OutlineId`, `AnnotId`, `FieldId`, `BlobId` (from store.rs) and
  `Local`, and every payload spec. sheaf-doc re-exports them.
- **sheaf-core is split and renamed `sheaf-util`** (§6.1):
  - `Rotation`, `PageSize` and `DestView` go to sheaf-edit;
  - `PageIndex` goes to sheaf-render;
  - the checksum and file reading stay, as sheaf-util;
  - `publish = false` goes on every crate outside the published set;
  - sheaf-ui's unused dependency goes.

  This happens in this slice, with the move, because the move already
  rewrites the same `use` lines. The README, `docs/document-model.md`
  and this plan's later slices say sheaf-util; earlier slice notes
  stay as written.
- **`Tx` and `ExportTx`.** `Document::lower_cx(actor)`; caps checked
  per resolved target before lowering; `Tx::check`.
- **Lowering:**
  - lowering reads, write befores and decision reads recorded by
    value;
  - cascades to a fixed point;
  - typed `EditError`s;
  - locals and `created`;
  - host-owned attributes, with identity tokens literal for plugin
    actors;
  - describe and `Step.effects` per op;
  - `Notice`s;
  - progress and cancel.
- **Inserts:** `InsertPages { pages: RangeSet, name }` with form
  instances. Positions resolve against the pre-op state.
- **`Document::commit` at the cursor;** stale bases arrive in slice 12.
  `DocView::commit` returns `Result`.
- **`SourceId = BlobId`.** Registrations go. Ops name sources with
  `BlobSel`, records hold `Src`, and `FormInst { src, copy }` is per
  op. A derived copy's `derived` record moves to a side table keyed
  by `SourceId`, until slice 17.
- **Callers:** the app's call sites build Ops, and UI macros read
  through `Tx::reader`.
- **Combine** is one Tx on the first file's document. It is still
  written and reopened, until slice 19.
- **Job amends** go through a Tx.
- **Transitional:** the WIT 0.2/0.3 → Op adapter, and an internal
  `SetPageBase`.
- **Deletes:**
  - `Edit`, `compute`, `double_option` and `EditOutcome`;
  - `OutlineInsertTree` and `CommentsDelete`, and `OutlineMove`'s
    `with_pages` compute arm, re-expressed as `MoveBookmarks`
    lowering that also puts `Order`;
  - `cascade_delete`, `bring_annots` (slice 8's, which
    `import_marks` became) and `merge_outline`;
  - the app's id-guess sites;
  - `sign_marks`, `keep_unseen` and `annot_from_wit`'s host filling;
  - `form_notice`, `script_sources`, `scripts_mixed`, `signed_from`
    and `next_copy`;
  - `take_in_fields` and `copy_fields` as Document methods, and
    `name_apart` (slice 8 folded `bring_fields` into
    `take_in_fields`);
  - combine.rs's `insert_runs` and its one-version-per-mark flow;
  - the registration layer: `SourceId(u32)`, `add_source`,
    `register_source`, `unlogged_sources`, `next_source`,
    `Version.sources` and `Document::edge`'s source list;
  - describe.rs's 21-arm match;
  - the `sheaf-core` crate (split and renamed, above).
- **Checks:**
  - A serde round trip, plus a committed schemars snapshot that
    rejects additions without a default.
  - One test per cascade rule.
  - A batch with locals: a bookmark `$1` with three children, two
    connected new marks, and a reply to a new mark.
  - Unknown ids are errors in Move, Rotate, Delete, Duplicate and
    `ClearOcrText`.
  - The new-bookmark selection regression.
  - A `MoveBookmarks` with `with_pages` moves the pages its bookmark
    spans (`outline_move_with_pages`, ported).
  - sheaf-edit builds for wasm32-wasip2, and `cargo tree` shows no
    sheaf-doc or sheaf-util under it.
  - A CI step like sheaf-image's (`ci.yml:65-71`): sheaf-util's
    `cargo tree -e normal` holds only itself and twox-hash.
  - A plugin update keeps `created`, `/NM` and tape rows.
  - The replay proptest and the Append ≡ Rewrite property run over
    `Op` generators.
  - **Forms:**
    - `a_files_pages_in_runs_join_one_form` (names `[a, a2, both]`,
      3 fields, 4 widgets);
    - `corpus_a_form_combined_with_itself_is_two_forms`;
    - `a_form_inserted_into_itself_goes_under_its_files_name`;
    - the notice-text tests (forms_saves.rs:306–336,
      forms_merge.rs:196–210).
  - Inserting `copy.pdf`, whose bytes equal `form.pdf`'s, names the
    parent `copy`.
  - `[delete-pages 1, rotate-pages {"pos":2}]` rotates the page now
    second.
  - Cap totality over (op, target kind) and over the actor table
    (§6.7), and a plugin actor deleting another tool's comment is
    `Refused`, as is `imported` from a plugin.
  - `Tx::check` on Delete with 7,000 pages selected takes under 1 ms
    and allocates no ids.

## Notes

Done. Edits are ops (proposals 20 and 25; bugs 8, 11, 12, 25, 27 and
   29): a caller says a change as `sheaf_edit::Op`s, a transaction (`Tx`)
   resolves each op's references against the state it finds, checks the
   actor's capability, lowers it into prims with the cascades, declared
   once, run to a fixed point, and `Document::commit` makes the ops one
   version at the cursor, with what they made (`Commit.created`), their
   lines, effects, cascades and typed notices. sheaf-edit is the
   vocabulary, types and JSON with a committed schema, and builds for
   wasm32-wasip2; sheaf-core is split, what stays renamed sheaf-util. A
   source is its blob (`SourceId = BlobId`) and each insert, replace and
   duplicate is a form instance of its own, so the registrations and
   slice 10's form-shift refusal are gone. The app, the plugin host
   (through the transitional WIT 0.2/0.3 → Op adapter), sheaf-ops, the
   command line and every test build ops, and Combine is one
   transaction. `Edit`, `compute`, `EditOutcome`, the registration layer,
   `form_notice`, `insert_runs`, the app's id guesses, `sign_marks` and
   `keep_unseen` are deleted. At 7,000 pages `Tx::check` of deleting every
   page takes 0.39 ms and draws no id; an AddMark and a full reorder, now
   transactions, take 0.022 and 1.0 ms; the Rewrite is 1.12× slice 8's
   writer on both fixtures; the corpus counts hold.
   - **sheaf-util and sheaf-edit** (step 1). sheaf-core is `git mv`ed to
 `crates/sheaf-util` and keeps `checksum`, `checksum_hex`,
 `Checksum`, `read_file` and `read_file_and_metadata`, twox-hash its
 only dependency. `crates/sheaf-edit` (0.1.0, published; serde and
 schemars, serde_json for tests) holds `ids` (`PageId`, `OutlineId`,
 `AnnotId`, `FieldId`, `Local` written `"$n"`, `BlobId` written
 `"b<32 hex>-<len>"` with `stem`/`from_stem`, and `SourceId =
 BlobId` from step 2), `page` (`Rotation`, `PageSize`, `DestView`,
 `TextStyle`), `geom` (`Pt`, `Box2`, `Vec2`), `tape` (moved whole;
 its calculator `Op`'s schema is `TapeOp`), `measure` (`Measured`,
 `Scale`, `points_per`), `text_layer` (`TextLayer`, `OcrWord`),
 `header_footer` (`HeaderFooterSpec`, `Margins`, `BatesSpec`),
 `labels` (`LabelStyle`, `PageLabelRange`), `mark` (`Color`, `Style`,
 `MarkKind`, `NewMark`, `MarkPatch`, `Imported`, `Anchor`), `refs`
 (`PageRef`, `BookmarkRef`, `AnnotRef` over one hand-written visitor:
 a number is an id, a string a local, an object `{"pos"}`,
 `{"path"}` or `{"nm", "page"}`; `BlobSel`, `PageSel`, `ScopeSel`,
 `RangeSet` written `"1-3,5"`), `patch` (`Patch<T>`: absent keeps,
 null clears, a value sets), `op` (the 21 ops, `Turn` as 90/180/270,
 `Sibling`, `Delta`, `BookmarkPatch`, `PageSource::File`, `Bring`)
 and `envelope` (`Envelope`, `VERSION` "1.0", its reader refusing
 another major or a later minor as soon as it reads `v`).
 `deny_unknown_fields` is on every op-layer type, and (the review's
 finding 1) on the moved `TextStyle`, `DestView`, `TextLayer` and
 `OcrWord`, which nothing stores as JSON. The record types
 `/SheafAnnot` stores inside its lenient `Annot` keep the serde they
 had: `Pt`, `Box2`, `Scale`, and `Tape` with its `Line`, `Row` and
 `TapeLook` (strict, a mark another sheaf wrote with a field more
 would read as another tool's comment; tape's own test reads old
 tapes). Their fields but a line's note and the look are required, so
 a misspelling there is still an error and only an extra field is
 ignored; `Measured` is a unit enum, already strict. sheaf-doc
 re-exports each moved
 type at its old path; `BlobId::of` became `store::blob_id`, and
 `TextLayer::from_engine`, `upright_box`, `unrotate_from_upright` and
 `line_of` functions in `text_layer`. `PageIndex` went to
 sheaf-render; ocr, ops, app, plugin and print use sheaf-util,
 sheaf-edit and sheaf-render; sheaf-ui's dependency went. Every crate
 but sheaf-edit and sheaf-plugin-api is `publish = false`. ci.yml
 gains "sheaf-util stays byte helpers" (its tree exactly sheaf-util
 and twox-hash) and "sheaf-edit stays types only" (no other sheaf
 crate under it, and `cargo build -p sheaf-edit --target
 wasm32-wasip2`).
   - **Sources are blobs, one form instance per insert** (step 2). The
 registration layer is deleted (`SourceId(u32)`, `SourceInfo`,
 `Document.sources`, `next_source`, `unlogged_sources`, `add_source`,
 `register_source`, `unregister`, `Version.sources`,
 `Snapshot.sources`, `PageBase.source`); records hold their `Src`,
 `PageBase { src, index }` and `FormInst { src: BlobId, copy }`.
 `Store::read_file(path, password) -> (Src, FileStamp)`,
 `store::file_name`, `Store::sources()` (what `find_stamp` and
 `find_file` look through, the primary first), `Document::new(store,
 primary, name, …)`, `Came::Read { path, stamp }` and `Came::Restored
 { path, base, pending }`. Every insert or replace makes a new
 `FormInst`, its copy the next its blob has in the state (copy 0 of
 the primary's kept for its own instance), named by the op; the join
 branch of `take_in_fields` and combine's `insert_runs` went (Combine
 issued one insert per file from here). The emitter indexes an
 instance by `inst.copy`, so `FormCopies`, `deps::Closure::form_shift`
 and `append::Refused::FormShift` are deleted, and the old FormShift
 test is inverted (`a_form_instance_keeps_its_objects_across_saves`).
 `deps::Closure::stale` reads a widget and a field as `inst.copy` of
 their blob, as the emitter does, and `Relinking`'s primary and the
 catalog it walks are `SaveInput.primary`'s.
 Derived copies' origins are a side table, `Document.derived:
 Arc<HashMap<BlobId, Derived>>` (`add_derived`, `derived()`), which
 `Snapshot` and `SaveInput` carry with the primary; `DerivedSource {
 src, derived, pages }` with `note` and (step 7) `bases`. Each
 `Version` notes once, at push and amend, the blobs its steps bring
 (`blobs`), so `undone_source_bytes` walks no step. Scripts count per
 instance. The app keys engines by `BlobId`, so the same bytes share
 one.
   - **The transaction** (`crates/sheaf-doc/src/tx/`, step 3).
 `Document::lower_cx(actor)` (the only constructor of `LowerCx`: the
 store, the primary, the `Arc<IdAlloc>`, `HostAttrs` and the actor),
 `Document::tx(actor)`, `Tx::{apply, apply_batch, check, read,
 reader, pin, grant, grant_named, state, set_page_base}`,
 `Document::commit(tx, CommitOpts)` and `apply_in_job(job, tx,
 label)`. `apply` resolves (`tx/resolve.rs`: `ResolvedOp`, the
 `Resolver`), checks caps (`tx/caps.rs`), lowers by family
 (`tx/{pages, bookmarks, marks, content, forms}.rs`) and settles the
 cascades (`tx/cascade.rs`), all or nothing; `apply_batch` is all or
 nothing over an im-backed checkpoint, and a canceled op leaves the
 Tx as it was. `check` resolves and checks caps only. Lowering reads
 are kept by value (`seen`), decision reads through `read`/`reader`
 (`Reader`: `order`, `page_at`, `pos_of`, `def`, `geom`,
 `annot_ids_on`, `marks_on`, `bookmark_pages`, `annot`, `bookmark`,
 `outline_roots`, `labels`, `form`, `field`). `EditError::{NotFound,
 Invalid, Ambiguous, Refused(Cap), NotReady, Cancelled, Mismatch {
 version }, Internal}` and `Notice::{FormRenamed, CopiesRenamed,
 SignatureNotValidHere, ScriptsOff, Unencodable}` with `notice_text`
 (`tx/error.rs`). `Commit { version, amended, created, effects, lines,
 cascades, notices }`; `Created { ops, locals }` with `made`, `pages`,
 `bookmarks` and `annots`. `Step { op: Arc<ResolvedOp>, prims,
 effects: StepEffects, line, cascades }`; `Version` records its
 `Actor` beside its author; `VersionView.edits` is `Vec<describe::Line
 { text, removes }>` with `actor`. Undo, redo and jumps return
 `EditError` (`Mismatch`, `NotFound` for a version no longer kept).
 `Document::set_identity(Identity { person, initials, date_format,
 time_format })` replaces `set_author`; `HostAttrs::now` formats the
 date and time with chrono.
   - **Lowering** (step 3), every slice-11 op, ported from compute's
 arms: ids and `/NM`s, dates from `HostAttrs.now` (one a
 transaction), the default author (the person, or a plugin's id for
 its marks), the identity tokens (`stamp_lib::fill`, moved from
 `sheaf_ops::stamps::fill`; `<<name>>` and `<<initials>>` left as
 written for a plugin), clamps and kind filters, `Imported`, a
 connector's partner, a reply's parent `/NM`, a stamp's and an
 attachment's stem and an attachment's size from its asset, and a new
 header's date and file. The form helpers are lowering's (`take_in`,
 `copy_forms`, `apart`, `instance_copy`, `turn_scripts_off`,
 `signed_in`, `scripted_instances`). `MoveBookmarks { with_pages }`
 puts `Order` (OutlineMove's arm). An insert's marks, links, comments
 and outline come from its source's facts by `Bring`
 (`bring_annots` and `merge_outline` as lowering). Insert, replace,
 duplicate and delete report progress by page. describe.rs is
 rewritten over `ResolvedOp`, its page numbers by `pos_of`;
 `describe::lines` collapses a run of one kind into one line, and the
 internal page base's line names its flow (bug 8). effects.rs gains
 `StepEffects { effects, pages, keys }` and `ClassifyCx::of_step`.
   - **Cascades and validation** (step 4). Seven rules in `Rule::ALL`:
 `page.annots`, `annot.thread`, `annot.partner`, `widget.field`,
 `page.bookmarks`, `page.labels` (`labels::anchored`: a range starting
 on a gone page starts at its range's next page, keeping `first`, or
 goes) and `page.generators` (`Generator::on_pages_left`; a header
 left on no page goes). One trigger pass a round (`Gone::of`, an
 exhaustive match on `Key`), rounds capped at `MAX_ROUNDS` (8,
 `Internal` past it). Validation moved into resolve, so `Tx::check`
 sees it; unknown ids are `NotFound` in every op (bug 11).
 `check_invariants` adds label anchors, the header's page scope,
 symmetric connector pairs and sheaf replies to a comment on their
 page; `open/annots.rs`'s `pair_connectors` and `save::subset` keep
 them. DuplicatePages draws mark ids across the op, renames a copied
 thread's replies to the parent copy's `/NM` (other tools' replies
 too) and pairs copied connectors (bug 12), and a copy joins the
 header's scope after its original.
   - **The Op JSON fuzz target** (step 4): `source::fuzz::op_json`
 (`fixtures`), `fuzz/fuzz_targets/op_json.rs`, `fuzz/op.dict` and the
 fuzz.yml matrix with a dictionary per target. It applies a mutated
 envelope to a four-page document with a form, every kind of mark,
 other tools' comments, bookmarks, labels and a header (one op at a
 time when the batch fails), then commits, undoes and redoes,
 checking the invariants each time. `write_fuzz_seeds` writes 57
 seeds (the fixture as one envelope, each op alone, and an
 `applying` envelope whose 49 ops all apply);
 `fuzz_bodies_survive_mutated_fixtures` mutates them as JSON and as
 bytes. Step 1's fixture moved to `sheaf_edit::fixture` (its
 `fixtures` feature).
   - **sheaf-doc on ops** (step 5). `tx/fixture.rs` (`fixtures`):
 `Document::{commit_ops, commit_ops_in_job, commit_page_base}`,
 `hold_incoming` and the op builders every test and other crates'
 tests use. `exchange::import` builds ops (`Imported.ops`, `grants`,
 `orphans`, `ops_in(tx)` pinning `held`): one of our marks is a
 `PatchMarks` by `Nm { nm, page }` (by id when the name is on that
 page twice) with `mark_patch` and `imported`, another tool's
 holdable comment on the same page `AdoptAnnotations` then that
 patch, anything else `AddMark` with `imported`, replies after their
 parents; the app's import commits as `Actor::Job(Import)`, `sheaf
 comments import` as `Actor::Cli`. history_tests' generators are
 rewritten over ops (22 kinds: the 21 ops and the page base), naming
 targets by id, position, local, `/NM` and title path, a BATCH one
 Tx with each op generated against what the ones before it made, a
 JOB through `apply_in_job`, and the other file carrying our marks
 (`with_ours`); append_rewrite_tests' `Run::over` and `act` use them.
   - **The app** (step 6). `DocView::commit(tx, opts) -> Result<Commit,
 EditError>` shows the error and reveals `Commit.created`'s bookmarks;
 `user_tx`, `apply`/`apply_batch` (an `Actor::User` op or batch,
 `Option<Commit>`) and `submit(tx, ops, label)`. The id guesses are
 `Commit.created`: a new mark, reply or paste selected by its id, the
 note, free-text, callout, replace and insert editors opened on it
 (`pending_edit`), New Bookmark through `new_bookmark_op` and
 `made_bookmark`. Adoption is `AdoptAnnotations` (the comment keeps
 its id), a drag `MoveMarks { by: Delta::View }` at mouse-up, a
 handle, style or text edit a `PatchMarks` from
 `tx::mark_changes(was, now)`. Set Status, Reset Status, Bookmarks
 from Pages, the pattern panel, Color by Level, the tape and paste
 read through `Tx::reader()` and submit one batch. An insert reads
 the file and calls `tx::ready_to_insert` on the background thread,
 and the UI thread pins what it decoded in an `Actor::Job(Insert)`
 Tx. Crop, strip and watermark go through `DocView::rebase`
 (`Tx::set_page_base`, then their header op). The History panel has
 a "Removes" badge and says "Plugin <name>". `finish_edit`'s removal
 notice adds the cascades' fields and replies.
   - **Plugins** (step 6). `api::shared::Adapter` is the transitional
 WIT 0.2/0.3 → Op adapter: page edits by `PageSel::ids` (a turn of
 R0 is no op), the five outline edits as AddBookmark (locals for a
 tree), PatchBookmarks, MoveBookmarks and DeleteBookmarks, AnnotAdd
 as AddMark in PageSpace (a reply threaded by `/NM` on its page),
 AnnotUpdate as a `PatchMarks` naming only what the record says
 (kind, color, opacity, stroke, contents; fill, dashed and cloudy for
 0.3; what the API doesn't show of a kind taken from the call's
 snapshot), AnnotDelete as DeleteAnnotations and ConnectorAdd as
 AddConnector. A mark's stamp or file stem goes in `EditBatch.held`
 as `BlobSel::Asset(n)`, and `EditBatch::lowered(doc, actor)`
 resolves it with `find_stamp`/`find_file` into `Tx::grant` and
 lowers the batch as `Actor::Plugin { id, version }`; the app commits
 it through `DocView::commit`. `annot_from_wit` became `said()`, with
 no author or dates.
   - **sheaf-ops, sheaf-render and the command line** (step 7). Combine
 is one `Actor::Job(Insert)` Tx on the first file's document:
 `DeletePages` for its pages left out, one `InsertPages { pages:
 RangeSet, name }` a further file with `Bring { marks: true, form:
 New, outline: None }` (incoming blobs pinned through
 `ready_to_insert`), one `AddBookmark` a bookmark naming its pages
 and parent by what the earlier ops made (`OpResult.made`), one
 commit "Combine"; still written and reopened. `blank::write` is one
 `Actor::Cli` Tx, `scan::add_text` commits `SetOcrText` as
 `Actor::Job(Ocr)`, `DerivedSource::bases()` yields what
 `Tx::set_page_base` takes. The crop, comments, forms, interop,
 corpus and gate tests in sheaf-ops and five sheaf-render tests use
 the fixture's ops.
   - **The bugs:** 8, a derived flow's rebase is said as its feature, not
 "Change the content"; 11, every reference resolves or is an error
 (a bookmark's target, a label range's start, the header's pages
 among them), and the invariants check label anchors and the header's
 scope; 12, the cascades are declared once, so a deleted page's
 fields, label range and header pages follow it, and a duplicate's
 threads and pairs are its own; 25, every id guess and
 Combine's page lookups are what the ops made (`Commit.created`,
 `OpResult.made`), and a drag or handle edit commits a `MoveMarks`
 delta or a `PatchMarks` of what changed at mouse-up rather than a
 whole record cloned at mouse-down; 27's slice-11 half, `sign_marks`
 and `keep_unseen` gone, a plugin's batch one transaction of its own
 actor, its marks its own (the version check is slice 12's, a submit
 that says what it made WIT 0.4's); 29's, notices typed in
 `Commit.notices`, Combine's joined into its report, and a form
 instance an op's.
   - **Deleted,** each confirmed dead by `cargo check --workspace
 --all-targets` and clippy: `model::Edit` (every variant, with
 `OutlineInsertTree`, `CommentsDelete` and `RebasePages`), `Rebase`,
 `NewOutlineItem`, `double_option`, `Document::apply` and
 `apply_batch`, and the Edit→Op shim that let steps 3–6 compile
 (tx/shim.rs); `compute`, `EditOutcome`, `form_notice`,
 `unencodable_notice`, `Damaged`, `NotReady`, `check_ready`, the
 Document methods `take_in_fields`, `copy_fields`, `bring_annots`,
 `script_sources`, `scripts_mixed`, `scripts_apart`, `scripted`,
 `tree_from_items` and `set_author`, and the model functions
 `cascade_delete`, `merge_outline`, `name_apart`, `next_copy`,
 `signed_from`, `holds_same`, `fresh_annot_id`, `outline_insert` and
 `outline_unlink`; describe's 21-arm match; the registration layer,
 `FormCopies`, `form_shift`, `FormShift`, `graph::load` and
 `DerivedSource::{info, rebases}`; combine's `insert_runs` and its
 version per mark; the app's id-guess sites, `Incoming`,
 `start_annot_editor`, `pending_edit_last`, set_status's
 `status_edits`/`reset_edits` and `DocEvent::Edited`'s `outcome`;
 `sign_marks`, `keep_unseen`, `Signer`, `edit_from_wit`,
 `annot_from_wit`'s host filling and `EditBatch.edits`;
 `exchange::Imported.edits`; the sheaf-core crate.
   - **Ledgers** (step 8). retired.rs gains 73 slice-11 rows (each step's
 names, and step 8's reconciliation: `debug_assert_ours`,
 `check_ready`, `tree_from_items`, `Document::scripted`,
 `outline_insert`, `outline_unlink`, `fresh_annot_id`,
 `set_status::{status_edits, reset_edits}` and
 `DocEvent::Edited::outcome`) and `QUALIFIED_ONLY` seventeen names
 (`Ids`, `from_engine`, `sources`, `range`, `info`, `load`,
 `registered`, `edit`, `Damaged`, `NotReady`, `edits`, `Edit`,
 `NewOutlineItem`, `apply`, `apply_batch`, `scripted`, `outcome`),
 each a name the new code or another crate uses again; temporary
 probe files confirmed every new row catches the old definition,
 call, path, method or field form it should, and the ledger's
 self-tests pass. The transitional ledger marks the registrations
 (3→11), compute adapted (8→11) and the form-shift refusal (10→11)
 deleted, the job amends' slice-11 part done, the derived record a
 side table by `BlobId`, the generated-cache stem resolvers moved
 before lowering, and states the WIT adapter (11→20), the internal
 `SetPageBase` (11→17), commits only at the cursor (11→12), Combine
 one Tx still reopened (→19) and Remove Header/Footer through
 `Tx::set_page_base` (→17). No lint ban lands.
   - **Docs:** `docs/document-model.md` gains "Ops and transactions
 (M10.5 slice 11)" after Undo/redo (the Op and Tx sketch, the two
 crates, Op JSON and the schema, references, the three stages and
 `check`, actors and caps, what the host owns, the cascades,
 validation and the invariants, steps, lines and effects, commits and
 `created`, sources as blobs and form instances by op, the callers
 and the adapter, the gate numbers); the Core idea, Page content's
 notices and stems, Undo/redo (`Version.actor` and `blobs`, the Step,
 `Mismatch`, lines, the generators, the costs), Saving's
 registrations and `FormCopies`, slice 10's form-shift passages (now
 history), the Decisions' history accounting and Text layers' ops
 and job say what changed. README: the crate box (sheaf-doc's ops,
 sheaf-edit, sheaf-util), the Undo row and the History panel's row.
 docs/plugins.md: a plugin's marks are its own and its tokens stay as
 written (step 6), and a batch is the plugin's, so what needs the
 user's approval is refused (step 8).
   - **The 7k harness** (step 8): `Tx::check` of `DeletePages` naming
 every page by id, the median of 21, asserted under 1 ms on every
 fixture (`CHECK_GATE`; the scanned only printed it until the review,
 findings 6 and 14), and of `DuplicatePages` naming every page, which
 lowered would draw 7,000 page ids, with the allocator's counters
 asserted equal before and after both (`Document::ids_next` is
 `fixtures` now); in the history bench one transaction of 7,000 `SetOcrText`,
 lowered and committed as an OCR job's version, asserted one History
 line (no gate). The history gates (AddMark < 1 ms, a full reorder
 < 2 ms, heavy adds ≤ 2× blank) go through transactions and stay
 asserted.

   Deviations from the plan (the planner's list first, as resolved):
   - **The Mark record is kept** (`Mark { annot: Annot, src, file,
 stamp }`): replies name their parent by `/NM`, the connector's
 partner is an `AnnotId` in `AnnotKind::Connector`, and stamp and
 attachment stems are in the kind, where §3.6 has `reply_to` and
 `partner` by id and a kind without host fields. `NewMark.kind` is a
 new `sheaf_edit::MarkKind` (AnnotKind's creatable variants without
 partner, parent and stems), which lowering fills from `AddMark`'s
 `reply_to`, `AddConnector` and `NewMark.asset`; threads and the
 duplicate's remap match by `/NM` on the page, as `thread_of` does.
 The public surface is final, so the record's restructure is
 internal: recommended as a follow-up (For Greg).
   - **Ops deferred to the slices that lower them,** a scope deviation:
 `SetPageBoxes` (15), `SetWatermark`/`RemoveWatermark` with
 `WatermarkSpec`/`WatermarkContent` (16), `RemoveHeaderFooter`'s
 `include_authored` (16–17), `FlattenAnnotations`,
 `PageSource::{Blank, Images}`, `Transform` and `ExportTx` (19; their
 only consumer is `export::run`). Shipped now, they would parse and
 then fail to lower; the schema's additive rule makes adding them
 later safe. Refs and selectors ship now (`PageSel::{Span,
 Positions, Bookmark}`, `BookmarkRef::Path`, `BlobSel::With`), since
 resolving them is this slice's and they're tested; their producer is
 slice 19's command line.
   - **API whose consumers are later,** deferred with them: `Tx::preview`
 and `Preview`, `read_render`, `mark_handed_out`,
 `EditError::{Conflict, GroupEnded}`, `CommitOpts.{base, stale,
 group}` and `Commit.change` (12), `JobKind::Trim` (15).
 `EditError::Content` has no producer until an op lowers content
 (16–17), though slice 6 asked this slice to map `ContentError`; its
 other half, a stem that names nothing, is `NotFound` at resolve.
 sheaf-plugin-api doesn't depend on sheaf-edit until WIT 0.4 (20);
 only existing crates got `publish = false`. The app has no
 `can_apply` yet, so `Tx::check`'s consumers are its unit test and
 the gate. The envelope is defined in sheaf-edit as the schema's root,
 read by the schema test; its producers are `sheaf edit` (19) and
 WIT 0.4 (20).
   - **Geometry is the kind's:** the design's `NewMark.geometry` would
 duplicate what each kind carries, so `MarkKind` carries it and
 `MarkPatch.kind` replaces the kind whole, geometry included, beside
 the named style and contents fields.
   - **Effects:** `effects::Effects` stays `Copy { flags, sig }`, which
 `judge` relies on; a step's are `StepEffects { effects, pages, keys
 }` (`Arc<BTreeSet>`s), judged on the step alone
 (`ClassifyCx::of_step`), with no whole-document `shown` walk: a page
 or layer a step took away counts as removed even if another page
 also shows it. `Commit.effects` is the union of its steps'.
   - **Names:** the op's label range is `PageLabelRange` (labels.rs keeps
 `LabelRange`, 0-based positions); tape's calculator `Op` stays
 `sheaf_edit::tape::Op`, its schema `TapeOp`.
   - **The join branch went in step 2** with page-set inserts, not step
 3: with `FormInst.src` a `BlobId`, the same bytes inserted twice
 would have joined one instance. Inserting a file twice gives two
 forms renamed apart (materialize_corpus's 1,098 still write).
   - **Scripts per instance** (§3.7): `scripted_instances` counts
 instances, not distinct sources, so every insert keeps today's
 behaviour and a scripted page's duplicate now turns scripts off
 (`a_scripted_page_s_duplicate_turns_scripts_off`).
   - **`Damaged` and `NotReady`** are `EditError::Mismatch { version }`
 and `EditError::NotReady`; the History panel and the app's open
 follow.
   - **`SourceInfo`'s parts:** an instance's name is the op's `name` (the
 primary's from its open path); `Derived { from: Src, pages }` is in
 the document's table by `BlobId`; the record's path travels in
 `Came`. `BlobId::of` needs the checksum, so it's sheaf-doc's
 `store::blob_id`; `BlobId` had no serde, so its string form changes
 no persisted format.
   - **Identity tokens** moved into sheaf-doc (`stamp_lib::fill`), since
 lowering fills them; `HostAttrs` carries the person, the initials,
 now as a PDF date, and the date and time as dynamic stamps write
 them. `Document::set_identity` takes an `Identity` struct; an
 invalid strftime format from settings falls back to the defaults.
   - **Stems become grants:** WIT 0.3 names stamps and attachments by
 stem and the design refuses a plugin's raw `BlobSel::Id`, so the
 host resolves a stem (the generated-cache arm included until 19)
 into a grant on the Tx (`Tx::grant`, `BlobSel::Asset(n)`). Grants
 live on the Tx, not in `LowerCx` (an `Arc` made only by
 `lower_cx`); `Grant` is `Blob(BlobRef)` or `Stamp { stem, form }`,
 since a `FormRef::Base` has no stem.
   - **Attachments decoded before lowering:** `resolve::incoming` and
 `ready_to_insert` decode and intern what an inserted file's marks
 hold, off the UI thread in the app, and the caller pins them. An
 attachment whose decoded bytes' id differs from the mark's stem now
 holds no file (compute held the mismatched blob).
   - **retired.rs:** moved helpers got new names where a bare call of the
 old name would hit (`thread_of`, `outline_from_flat` and `nest`
 moved under their names and are live, so they have no rows);
 `Edit`'s variants are retired with their enum, since a variant row
 would hit WIT's `Edit::OutlineInsertTree`; the sheaf-core crate's
 retirement is its absence and the CI tree step, as a `sheaf_*`
 prefix is stripped before matching.
   - **The new-bookmark guess** was a sibling-index lookup
 (outline_panel.rs), not the pre-order bug 25 describes, and right
 whenever the edit succeeded: with it put back,
 `a_new_bookmark_is_the_one_selected`'s refused half fails (it
 selected an existing bookmark, which the panel would rename); a
 pre-order guess fails its success half.
   - **The 7k harness's history rows** stay O(what the op names):
 describe numbers pages by `pos_of`, and `undone_source_bytes` reads
 each version's blob set noted at push.
   - **fuzz/Cargo.lock** was updated with `cargo update --workspace
 --offline` (sheaf-util, sheaf-edit, and sheaf-doc's `bitflags`, which
 an earlier slice had left stale); the fuzz build can't run here.

   Deviations found while building:
   - **Step 1:** `NewMark.turn` (default R0, left out of JSON at R0), the
 rotation a mark reads upright in, which producers set (markup on
 turned text, marks on a rotated page, paste); `MarkKind::
 FileAttachment` has no `size` (lowering sets the asset's length);
 `MarkKind::Reply {}` is a struct variant so it refuses extra keys;
 `BlobSel`'s Deserialize is derived (all its forms are objects);
 `RangeSet` is non-generic `u32`; `Turn`, `Sibling`, `Anchor`,
 `AddConnector { label, style }` and `HeaderFooterSpec` (no font
 until 17; date and file the host's) are shaped here; `Bring { marks,
 form: New, outline: None | MergeByTitle }`, comments and links
 always coming; `TextLayer::text()` reimplemented in sheaf-edit;
 `measure` moved only its types and `points_per`.
   - **Step 2:** `next_copy` reserved copy 0 of the primary's blob even
 when the state doesn't hold it, as `FormCopies` did, so today's
 flows' copies equal the old indexes and the corpus wrote the same
 bytes; deps' "other files" are the non-primary blobs, so a
 self-insert isn't another file (the emitter already worked by
 blob), and the emitter's `others` are in `BlobId` order (it matters
 only for `/DR` renames with two or more other files whose fonts
 clash; no test or corpus case has one); `brought()` counts a page's
 source only when a prim changes which source it shows; a non-primary
 first page's FDF `/F` and the untagged note name the blob's first
 origin (`Blob::origin_name`); `graph::load` became `graph::eager`;
 `RebasePages` looked each source up once an edit (crop's edit went
 from 12.6 to 16.6 ms without it). Test changes:
 `append_rewrite_widgets_whose_objects_moved_rewrite` appends now
 (a page replaced by its own page is an instance of its own) as
 `append_rewrite_a_page_replaced_by_its_own_page_is_a_form_of_its_own`;
 `missing_objects_on_a_signed_file_ask_and_write_nothing` drops a
 mark's key from the record (`forced_lacking`) instead of relying on
 FormShift; the waiting-registration parts of three history tests
 went with unlogged sources.
   - **Step 3:** cascades share one trigger pass a round (`Gone::of`),
 `annot.thread` finds a whole thread in one run, `Cascade` is `{
 rule, objects }`; the app's "removed N pages and M bookmarks" notice
 compares counts before and after (`Had`), since a delete's pages and
 a bookmark delete's subtree are the ops' own work; replacing a
 header keeps its frozen date and file, and `RemoveHeaderFooter`
 narrows the scope (All becomes a page list); `ReplacePages` ignores
 `bring.outline` (Replace never merged an outline, and refusing the
 default would refuse default Op JSON); there's no `wrote` set (a
 slot is unwritten while it holds its base value); `locals` and
 `seen` are im collections; a `PatchMarks` that changes nothing, or
 a `MoveMarks` by zero, makes no prim; the Edit→Op shim translated
 every old edit for steps 3–6, among them `CommentsDelete` to the
 foreign comments' ids and an `AnnotAdd` naming another tool's
 comment to Adopt plus a patch; `AdoptAnnotations` resolves a
 `PageStamp` stem through `find_stamp`, its generated-cache arm a
 file read inside lowering for that one case until 19; describe
 collapses runs at view and commit time, the last line of a run
 shown for labels and header ops; `Version.author` stays beside
 `actor`.
   - **Step 4:** two refusals beyond the list (moving a thread's mark to
 another page, and patching a connector into another kind, are
 `Invalid`, since threads are by `/NM` per page and pairs must stay
 pairs); an undeclared local is `NotFound`; the connector invariant
 is that both ends name each other, which open's `pair_connectors`
 makes true of files; the reply-parent invariant is narrowed to
 sheaf's Reply and Status marks (files hold orphans and cross-page
 `/IRT`; the debug-built corpus runs found no file that breaks the
 narrowed form); `page.labels` keeps a re-anchored range's `first`,
 so its pages renumber within it, as for a middle page deleted;
 `page.generators` removes a header left on no page; `save::subset`
 produces states that keep the new invariants, so an extract whose
 label range started on a page left out changes; the fuzz body is
 `cfg(any(test, feature = "fixtures"))` and sheaf-edit lists itself
 as a dev-dependency for its own tests; the 8-round guard is a unit
 test of `rounds()`; three proptest regression seeds were kept (the
 review deleted them: see Found).
 materialize/deps.rs changed beyond the list: `Deps::annot` writes
 the page when an annotation becomes or stops being a Replace mark,
 since its StrikeOut is listed beside it (found below).
   - **Step 5:** comment import grants each asset by its stem
 (`BlobSel::With`), since a stamp found as a source's form can't be
 named by id; a target is by `Nm { nm, page }` unless that page has
 the name twice (then by id, since `Nm` would be `Ambiguous`); a
 reply whose comment is neither on its page nor in the import is
 left out and counted (`orphans`, "N replies left out"), where the
 old import kept it pointing at nothing; another tool's comment is
 matched only on the imported comment's own page and only if
 holdable; `DocView::commit` came ahead of step 6 for the import;
 BATCH applies each generated op to the Tx and skips the ones that
 fail; OCR jobs run as `Actor::Job(Ocr)`; move-bookmarks falls back
 to add-bookmark with fewer than two bookmarks; delete-pages never
 selects by span or bookmark (a bookmark's span could be every
 page); `a_run_applies_every_kind` retries a kind up to 8 times; the
 no-op table follows the ops (no no-op for inserts, replaces and
 adds; empty lists for the list ops; the same layer `Arc` for OCR;
 a set-page-base Tx of no pages); `edit_serde_round_trip` went with
 `Edit`'s serde.
   - **Step 6:** the plugin-update Check's test is in sheaf-plugin's
 api/mod.rs (`a_plugin_update_keeps_what_it_cant_see`), since the
 WIT bindings are crate-private there; the app's plugins.rs holds the
 identity test (`a_plugins_marks_are_its_own_and_never_name_the_user`);
 a plugin's marks are authored by its id; `EditBatch` is `{ label,
 ops, held, invalid }`, a tree whose depth skips a level carried as
 the error `lowered` fails with; AnnotDelete and AnnotUpdate ignore
 the record's page; an update's stem is ignored (the old shim refused
 a changed one); a plugin connector's ends read upright in their
 page's rotation now; `DocView::apply`/`apply_batch` return
 `Option<Commit>` (the error shown centrally); status replies write
 "{state} set by <<name>>"; "Delete comment" deletes by id every
 same-named comment of another tool's on the page;
 `reveal_new_bookmarks` runs on every commit's created bookmarks;
 Bookmarks from Pages and pattern hits append with `Sibling::Last`;
 additions used by callers: `PageSel::ids`, `From` for the refs,
 `Turn::by`, `Created`'s accessors, `Reader::{marks_on,
 bookmark_pages}`, `mark_changes`, `ready_to_insert`,
 `HeaderFooter::spec`, and the script harness's `"marks"` dump.
   - **Step 7:** Combine names its pages and bookmarks' parents by
 `OpResult.made`, not locals (`Local` is a `u16`, and a combine has
 no bound); Combine's marks now come through `InsertPages`'
 `Bring.marks` (a mark keeps its file object for copy 0, partners
 and reply names remapped, no adoption of a same-named comment), and
 a reply whose parent isn't on its page no longer fails it;
 `blank::write` commits as `Actor::Cli`; `DerivedSource::rebases()`
 became `bases()`; `holds_same` (the shim's last caller) got a row;
 materialize_corpus's insert-twice test keeps `Bring { marks: false,
 outline: None }`, what its old edit said; append_corpus's Theirs
 feature deletes by id every same-named comment on the page.
   - **Step 8:** `Document::ids_next` (and `IdAlloc::next`) moved from
 `cfg(test)` to `cfg(any(test, feature = "fixtures"))` for the gate;
 the check row names every page by id rather than `PageSel::All`
 (the costlier resolve; `a_check_lowers_nothing` covers `All`); the
 batch row is OCR's (`Actor::Job(Ocr)`, `apply_in_job`), on the
 history bench's 20,000-mark document; retired.rs's reconciliation
 added the ten rows above, with `scripted` and `outcome` qualified
 only.

   Measured (AMD EPYC 7302P, 32 threads, release builds unless marked;
   the machine shared with another workflow's agents, the load average
   given with each run):
   - **The 7k harness** (`gates_on_the_*_7k`, three whole runs at a load
 of 0.9–1.9):

 | | Synthetic | Heavy | Scanned |
 |---|---|---|---|
 | `Tx::check`, deleting every page by id (median of 21) | 0.386–0.390 ms, asserted < 1 | 0.388–0.390 ms, asserted < 1 | 0.388–0.392 ms, asserted < 1 since the review |
 | `Tx::check`, duplicating every page by id (the review's; one run at a load of 27) | 0.392 ms | 0.392 ms | 0.388 ms |
 | ids drawn by them | none (asserted) | none (asserted) | none (asserted) |
 | incremental save, one mark | 20.1–20.6 ms | 38.3–39.7 ms | 21.1–21.5 ms, asserted < 50 |
 | crop all pages, the edit | 14.4–15.8 ms | 14.1–14.6 ms | 14.3–14.5 ms |
 | geometry walk | 47.6–47.8 ms, asserted < 100 | 74.7–88.6 ms, asserted < 250 | 60.6–61.0 ms |
 | ready | 53.9–54.5 ms | 344–356 ms, asserted < 1 s | 459–485 ms |
 | Rewrite, in the fixture's process | 148–153 ms | 657–681 ms | 543–650 ms |
 | Bates apply, all pages | 133–139 ms | 194–197 ms | 77–92 ms |

 The check is resolving 7,000 page references (each looked up, then
 the list made unique) and the capability; the heavy's 21,000
 annotations cost it nothing, since DeletePages' capability reads no
 annotation. Beside them, ungated: the AES-256 copies' one-mark save
 30.8–31.5 ms (synthetic) and 41.3–41.9 ms (heavy) and their Rewrite
 200–206 and 1,025–1,112 ms; the recovery sidecar of one mark 18.4–18.9,
 46.8–47.6 and 25.8–26.0 ms (1.21–1.33 KB); a one-page form inserted
 and appended 96–97, 478–492 and 1,087–1,127 ms (the scanned's 1,576
 MB added, `forms::check_latest` reading the whole file, as slice 10
 found). Nothing gated moved past its gate. After the review's fixes
 (one run of the three, load 27 at the start): the delete check 0.390,
 0.388 and 0.390 ms on the heavy, scanned and synthetic, every gate
 passing.
   - **History** (`history_at_7k_pages_and_20k_marks`, two runs; slice
 10's in brackets): AddMark 0.022–0.023 ms [0.015], a full reorder
 0.99–1.00 ms [0.74], each now a transaction committed (a `Tx` made,
 its host attributes taken, the op resolved, lowered, described and
 classified, then committed); asserted under 1 and 2 ms. One
 transaction of 7,000 `SetOcrText` lowers in 38.3–38.5 ms (5.5 µs an
 op) and commits as the OCR job's version in 5.7–5.8 ms, its one line
 "Recognize text on 7000 pages" (before the slice, a batch's line per
 op walked the pages, 35–50 µs each). 1,000 AddMarks on the heavy 7k
 (21,000 held) 0.154 ms against 0.141 on blank pages (1.09×; the last
 hundred 0.200 against 0.192), asserted ≤ 2×. Ready on 20,000 sheaf
 marks 517 ms (< 1 s); their Rewrite 1,459 ms. 200 OCR versions of
 1,000 pages: 4,115 MB of steps and 4,959 MB of RSS growth (slice 12
 bounds them); 4,254 MB of steps counted after the review's finding
 11 (each step's op, effect sets and cascades), RSS the same. After
 the review (four runs, load 1.4–1.8): AddMark 0.022 ms, a full
 reorder 0.94–0.99 ms, the 7,000-op transaction lowered in 38.4–38.7
 ms and committed in 5.66–5.73 ms.
   - **The Rewrite against slice 8's writer**
 (`rewrites_on_the_synthetic_and_heavy_7k`'s part; nine processes of
 each writer, run alternately, slice 8's from its own build's binary,
 each process's time the median of three runs; load 0.85–1.05):

 | | Synthetic | Heavy |
 |---|---|---|
 | slice 8's writer today | 134.6 ms (131.5–137.6) | 682 ms (584–712) |
 | `materialize`, slice 11 | 150.0 ms (145–152); 50.6 MB added | 790 ms (771–826); 252 MB added |
 | ratio to the gate's constant (134 and 707 ms) | 1.12× | 1.12× |
 | ratio to today's slice 8 median | 1.11× | 1.16× |
 | RSS added, to slice 8's 61.8 and 250 MB | 0.82× | 1.01× |

 No flag: both under 1.2× the gate's constant, the gate at 1.3×.
 Slice 8's heavy times are bimodal (584–590 or 700–712 ms a process),
 as slice 10 saw, so the ratio to its median moves with which mode
 more of its processes fell in; the per-pair ratios were 1.05–1.14
 (synthetic) and 1.10–1.35 (heavy). Steps 2 and 3 measured 151 and
 767 ms, slice 10 149 and 779 ms; nothing on the Rewrite path changed
 in slice 11 but the source lookups by blob and `inst.copy` in place
 of `FormCopies`, which the corpus shows write the same bytes.
   - **The corpus** (../pdf-corpus), every count as slice 10 left it:
 materialize_corpus's identity 4,389 of 4,392 written, 2,269,722
 primary objects surviving, 919,479 copied as they were, 436 changed
 by `keep_only`, 0 failed; 2,901 forms without their first page, 0
 failed (551 findings of the allowed kinds); every page duplicated,
 4,389, 0 failed; inserted twice into the 1040, 1,098, 0 failed (two
 instances renamed apart now, where one registration's instance was
 joined); four times, 0 differ; the 2932995 logo, 0 of 2,037,600
 bytes differ. append_corpus: 4,389 marked as planned, 3,770
 appended, 259 rewritten (211 RemovalCheck, 34 BaseObjectsMissing, 14
 BaseUnsound), 360 asked about signatures, 0 failed, 0 derived vetoes;
 the oracle finds text gone from 2,899 of 2,901 page-delete Rewrites
 (0016064 and 0112654 as before); the nine signed files as §9.3 says.
 The ignored tier, deleted means deleted over every file, six
 features both ways, 0 failed: the first Append declined [28, 68, 68,
 68, 0, 30] (a page: 17 BaseUnsound and 11 BaseObjectsMissing; a
 mark, a reply and a mark's text: 30 and 38; another tool's comment,
 30 files: none; a bookmark: 30 BaseUnsound), a page's content run
 found in 2,031 Rewrites and 2,022 Appends (599 s in the test profile
 at a load of 21–71).
 encrypted_corpus: 573 encrypted files, the Rewrite keeps 573 and the
 Append 572 (0630899 declined, its table rebuilt). Step 4's label
 re-anchoring changes the bytes of 246 of the 2,901 first-page-delete
 outputs (each a file with page labels whose first range started on
 the deleted page, now starting on the next); `widget.field`'s removal
 was hash-checked byte-identical over all 2,901; no count moved in
 any step.
   - **Suites:** `cargo test --workspace` 1,139 passed and 31 ignored in
 53 test binaries and 13 doc-test runs (5.4 minutes once built, at a
 load of 0.5–37; slice 10: 1,095 and 30), sheaf-doc's unit tests 726
 and 3 ignored in 31.5 s; fmt and clippy (`-D warnings`, all targets)
 are clean; `cargo check --target x86_64-pc-windows-msvc` passes for
 sheaf-doc, sheaf-ops, sheaf-edit and sheaf-util with their tests and
 for sheaf-render and sheaf-print (sheaf-cli, sheaf-app and
 sheaf-plugin can't be checked here: onig_sys's, ring's and
 wasmtime-fiber's build scripts need MSVC's headers); `cargo build -p
 sheaf-edit --target wasm32-wasip2` and both of ci.yml's tree checks
 pass locally. The ignored tier in release, the 7k harness apart
 (`-- --ignored --skip 7k --skip 200_ocr`, with qpdf 12.3.2 and
 pdf.js 6.4.299 on node 24.22, and `SHEAF_FUZZ_SEEDS` set so the seed
 writer wrote op_json's 57 seeds), about 10 minutes at a load of 4–54:
 22 passed, 0 failed. The replay property at 4,096 cases and Append ≡
 Rewrite at 1,024, over the `Op` generators, took 61 s together;
 deleted means deleted 342 s, its counts as above; interop's `qpdf
 --check` is clean on 84 Rewrite and 14 Append outputs, three Appends
 onto each of 14 bases and a delete saved and undone on each of 14,
 and pdf.js reads the records file both ways (the Append half keeps
 the comment only its Rewrite half deletes); the OCR engines, PDFium's
 frames on every page and the lexer-vs-PDFium differential pass.
   - **The review:** the plan was reviewed before the steps started (its
 findings are the planner's list above, as resolved); no review step
 ran within the slice's eight steps. Each step checked its own new
 checks by switching their behavior off (they failed, and passed with
 it back): the new-bookmark selection with the old guess put back, the
 plugin update with the tape's keep removed and 0.2 naming `dashed`,
 the plugin identity test with the actor switched to `User`, and each
 fix below with the fix reverted; each cascade rule's test lowers its
 op again with the rule switched off and finds the effect missing. The
 review of the whole slice follows.

   Found:
   - **By the property at 1,024 cases, fixed in the slice** (each a test
 that fails with its fix reverted): `annot.thread` took the replies a
 `ReplacePages` brought in, whose parent's `/NM` was the removed
 comment's (a thread now starts only from names no annotation on the
 page still has, and `thread_of` stops at a name another annotation
 outside it still has); adopting a duplicated or inserted copy's reply
 named the file's parent, not the record's (`summary.parent`); an
 adopted replacement whose StrikeOut was deleted earlier appended
 without its new StrikeOut (`deps::Deps::annot` writes the page when
 an annotation becomes or stops being a Replace mark; a step-3
 regression); `RemoveHeaderFooter` of pages outside the header's
 scope turned an All scope into a page list and pushed a version (the
 no-op table); and the removal check's `says()` for another tool's
 reply left out the name it replies to, so adopting a reply,
 appending, undoing and saving as another file made the Rewrite
 oracle see a derived removal
 (`append_rewrite_a_reply_made_editable_then_undone_removes_nothing`;
 one line in materialize/removal.rs). Of the proptest regression
 seeds saved in the slice, only append_rewrite's `6dfbb569` is kept:
 step 5's change of the action alphabet made step 4's three replay
 unrelated runs, so the review deleted them (the bugs they found
 have tests of their own). Slice 10's fixes' append_rewrite seed
 (`b7c3ef0b`, a page base set and a forced Append in slice 10's
 alphabet) went for the same reason: over the `Op` generators it
 replays an unrelated run, and so did two that proptest saved while
 the rebase's fault checks failed the property on purpose. A change
 to a property's alphabet invalidates its saved seeds.
   - **A wrong widget `/P` after a replace, an Append and an insert,
 fixed** (not this slice's: it failed at 121d4a3 and with the slice 10
 fixes, 99c7685). Open `files(false)`, Save As (a Rewrite, the fifth
 page at its own object, 13), replace the fifth page by form.pdf's
 second and force an Append, which writes the replacement over 13, the
 number the file's map has for that page; insert the primary's fourth
 and fifth pages before it (copy 0 of the fifth is free again), and
 the inserted fifth page, the primary's object (`Emission::owned`),
 takes 13 at the next Append, the replacement a new number; but the
 closure didn't write what names the replacement by number, so its
 widgets' `/P` still said 13, the other page. In the files: after the
 forced Append the replacement's widgets (86–89) said `/P 13 0 R`;
 after the next, the tree listed the inserted page at 13 and the
 replacement at 94, the widgets unchanged. With the primary's first
 page inserted too, the section wrote form keys and the post-condition
 stopped the save ("the written form has widgets with the wrong /P",
 form.form1[0].kind/print/sig/state); without it nothing of the form
 was written, so the form wasn't checked and the Append wrote the bad
 file, which the property's reopen didn't see either (it didn't read
 `/P`). Inserted at the end, the replacement came first in the
 emission, kept 13, and `held` refused (a Rewrite). The fix, fail
 closed: `deps::closure` finds the pages the file has at the number of
 a primary page that another page is copy 0 of now
 (`Closure::renumbered`) and writes each as a page that shows another:
 with no place in the saved tree (spliced), its annotations (widgets
 with their fields), the bookmarks to it, the links to its source
 page, and its source page moved for `stale` (`Relinking::moved`), so
 a copy's reference to it the section doesn't write again refuses the
 Append. `number::appended` gives an owned key its own number whatever
 is emitted first (the key the file has there takes a new one), so the
 pages' order no longer decides; `append::held` lets another key leave
 an owned key's number only when it's a page of `renumbered` the
 section writes elsewhere (before, any key written elsewhere); and the
 section's form is checked whenever it writes a page at another number
 than the file has it at. The property's `Reopened` compares each
 page's `/Annots` entries whose `/P` is another page. Tests, each a
 planned Append that reopens as a fresh Rewrite does, the
 replacement's number changed and no `WrongPage` in the file:
 `append_rewrite_a_page_appended_over_a_primary_page_gives_its_number_back`
 (the sequence above),
 `append_rewrite_a_page_appended_over_a_primary_page_writes_its_widgets_again`
 (without the first page: the bad file) and
 `append_rewrite_a_page_appended_over_a_primary_page_gives_it_up_wherever_it_comes`
 (at the end: refused before, appended now). With deps.rs, number.rs
 and append.rs as they were, all three fail (the post-condition; the
 reopen's other `/P` on four widgets; a Rewrite); with the closure's
 annotations of a renumbered page left out, the new form check stops
 all three saves; with `number::appended` as it was, the end case is a
 Rewrite. The seed the property found it with used step 4's numbering
 of actions and was never saved, and over the `Op` generators it no
 longer replays the sequence. With the fix, the 1,024-case property
 passed four runs in release (64–67 s each); no corpus count moved
 (IDENTITY, MARKED, ORACLE, the DELETED tier's declined [28, 68, 68,
 68, 0, 30], encrypted); the workspace suite: 1,169 passed, 31
 ignored, 0 failed; clippy and fmt clean.
   - **The emitter's own field rule** (emit/form.rs's `gone` and
 `showed`: a field shown nowhere that showed as read isn't written)
 mostly duplicates `widget.field` now; step 4 hash-checked that the
 cascade's removal writes the same bytes over the 2,901
 first-page-delete outputs. Simplifying the emitter is left until a
 byte-identity check says it's safe.
   - **`/DR` renames are still decided at emit,** not at lowering and
 recorded in `FormDoc` as §6.5 says (records.rs's comment said
 "until slice 11"): the emitter merges the instances' files' `/DR`,
 in `BlobId` order, deterministically. Nothing reads `FormDoc`'s
 `/DR` yet; left for the slice whose appearances read it.

   The slice review (18 findings on the uncommitted slice; each fixed
   with a test that fails without its fix, unless said):
   - **Op JSON** (1, 3): `deny_unknown_fields` on `TextStyle`,
 `DestView`, `TextLayer` and `OcrWord`, `ops-1.0.json` re-recorded,
 `a_misspelled_field_is_refused` covering them; the record types
 `/SheafAnnot` stores stay lenient, as named above and in
 docs/document-model.md. The schema guard now walks the new schema's
 keys too: any keyword the old one lacked but words (`description`,
 `title`, `default`, `examples`, `$comment`) narrows, with eleven
 cases added to `growing_passes_and_narrowing_fails`. A property
 added to an open object (no `additionalProperties: false`) still
 counts as growth, as the test always said.
   - **The invariants after every op** (2): `Tx::lower` asserts
 `check_invariants` again in debug builds, as `Document::step` did at
 121d4a3; `a_run_of_one_kind_is_one_line` runs 700 ops in a debug
 build (7,000 took 13 s there, 0.2 s before), 7,000 in release.
   - **A plugin naming a blob by id** (4): `a_plugin_names_a_blob_only_by_its_grant`,
 a FileAttachment `AddMark` its cap allows, refused at resolve by id
 and made through `tx.grant`.
   - **The docs** (5): resolve's "never in lowering" names the adopted
 page stamp's generated-cache read as the exception until 19.
   - **The check gate** (6, 14): asserted on all three fixtures, and a
 `DuplicatePages` check of every page (7,000 ids if it lowered)
 asserted to draw none; the vacuous "order untouched" assertion went
 (`check` takes `&self`).
   - **`<<file>>`** (7): `HostAttrs.file` is the name of the file that
 holds the document (`Document::file`), the name it opened as until
 its first save; after a Save As a new header names the new file, as
 before the slice (`a_header_names_the_file_the_document_is_in`).
   - **`AdoptAnnotations` of a comment and a part grouped under it** (8)
 panicked (the comment's adoption removed the part). Resolve refuses
 a grouped part beside the comment it's grouped under, in either
 order, as `Invalid`, and `adopt` returns `Internal` rather than
 panicking if an id is gone; a grouped part adopted alone is still
 allowed, as before the slice
 (`a_grouped_part_is_not_adopted_beside_its_comment`). A deviation:
 the finding offered refusing every grouped part; one adopted alone
 never panicked and the old adopt allowed it.
   - **A cross-page reply adopted** (9) broke the reply invariant when
 its parent was one of sheaf's marks. Resolve refuses adopting a
 reply whose parent isn't on its page but is named on another, and a
 comment one of sheaf's replies on another page is to (it would make
 that reply's parent a mark elsewhere), as `Invalid`: the rule
 `AddMark` has (`an_adopted_comment_keeps_its_thread_on_its_page`). A
 reply whose `/IRT` names nothing is still adopted.
   - **The WIT adapter's bookmark locals** (10, 13): a tree gives a
 local only to a bookmark with others under it, and puts each top
 bookmark's subtree at `after`, the last first, so no bookmark is
 named to place the next: a flat list of any length takes none, and
 past 65,534 parents in one batch it's `Invalid`, never an overflow
 (`a_bookmark_tree_names_only_its_parents`: order, 70,000 flat,
 65,535 parents). The ops' order differs from the items' (subtrees
 reversed); their ids are drawn in that order.
   - **History accounting** (11): each step counts its `Arc`'d op
 (`size_of::<ResolvedOp>()` and the counts), its two effect sets
 (`set_bytes`: full nodes of eleven) and its cascades, and the op's
 heap counts a mark's kind (`MarkKind`, its points and rects),
 `PatchMarks`' kind, `Imported` and a named asset
 (`a_step_counts_its_op_and_what_it_made`). Without `once`: lowering
 makes these for its step alone (with it, the 7,000-op commit took
 7.1–7.3 ms against 5.7). `StepEffects.keys` stays, §7.1's previews'.
   - **Derived copies alike** (12): two pages whose copies were byte
 for byte alike were one blob, the second `Derived` replacing the
 first outside undo. A derived copy's catalog names what it's a copy
 of (`/SheafCopyOf << /Blob (stem) /Pages [...] >>`), so different
 `Derived` records never share a blob; `add_derived` debug-asserts
 it (`pages_alike_cropped_apart_stay_apart`, both ways: a link to
 each page still leads to it).
   - **Proptest seeds** (15): step 4's three stale seeds deleted (see
 Found).
   - **Tests** (16, 17, 18): `a_line_says_whether_it_removes` (a run's
 summed line, the last of label sets, the badge); every op's
 `op_name` against its serde tag over `every_op()`
 (`an_op_is_named_as_op_json_names_it`); cascade_tests and
 `every_op` use `fixture::header`, describe's and cascade's tests
 `tests::doc_of` (now `pub(crate)`).
   - **Rerun after the fixes:** the workspace suite (1,148 tests, 0
 failed), clippy and fmt clean, sheaf-doc and sheaf-edit checked for
 x86_64-pc-windows-msvc; the corpus default tiers exactly the
 baseline (IDENTITY 4389/4392, 2269722 / 919479 / 436; 2901, 4389,
 1098 and 4389×4 with 0 failed or differing; logo 0 of 2037600;
 MARKED 3770 / 259 (211, 34, 14) / 360, 0 derived vetoes; ORACLE 2899
 of 2901; encrypted 573 and 572, 0630899 declined) at a load of
 16–41; deleted means deleted over every file, both ways, 0 failed,
 declined [28, 68, 68, 68, 0, 30] (429 s, load 28–35); interop's
 qpdf and pdf.js tests pass (84 and 14 outputs clean, three Appends
 and a delete undone on each of 14 bases).
   - **Rebased onto the slice 10 fixes** (99c7685, which landed on main
 first): every fix and test of theirs kept, said in ops. Their tests
 build `Op`s with `tx::fixture` (`commit_ops`, `commit_page_base`
 with a crop's copy noted by `add_derived`), each assertion as it
 was: the strategy, the `Why`, which `Refused` fires, the reopen
 against a fresh Rewrite and the counts. The property over `Op`
 generators has the fixes' fixtures (`files_opening`'s open action,
 both files' push buttons with GoTo actions), their comparisons (a
 widget's action, where each `/Annots` entry goes, the open action),
 `forced` answering `found`, and `refusal()`. Where the new model
 changed what's right:
 - finding 7's insert-based button: the other file's pages, then its
   first page again, then its second. Each insert is a form instance
   of its own now, so the copy's button is the second instance's,
   read as copy 1, and goes to the second page's copy 1, which the
   third insert brings: the Append is refused (`Stale`) and the save
   is a Rewrite (`append_rewrite_a_copy_s_button_action_follows_its_page`;
   with the refusal off, the Append's button went to the third place
   and the Rewrite's to the fifth). Before, the pages brought in again
   joined the first instance, copy 0, and it appended. The first
   instance's case stays an Append with only the second page brought
   in again (`append_rewrite_a_copy_s_button_of_the_first_instance_appends`),
   but there the button's page and instance are both copy 0, so it
   can't tell a widget read as its instance's copy from one read as
   its page's (the review's finding). Three tests do, with a merged
   push button, a radio's widget apart from its field, and the
   field's own `/AA`:
   `append_rewrite_a_later_instance_s_button_follows_its_page`,
   `append_rewrite_a_later_instance_s_widget_action_follows_its_page`
   and `append_rewrite_a_later_instance_s_field_action_follows_its_page`,
   the other file's
   second page holding a text field, its pages brought in, then its
   second, first and second pages again: the first page's copy 1 is
   the third instance, copy 2, its action going to the second page's
   copy 2, which the last insert brings, refused. Read as the page's
   copy (deps.rs's widget walk, its field walk, or both) one, the
   other, and all three fail.
 - `append_rewrite_labels_start_at_the_next_page_of_their_range`
   deleted the sixth page, which the fixes' push button goes to:
   rightly refused (`Stale` of form1[0].print[0], whose action would
   name a page outside the tree). Its ranges start at the third,
   fifth and sixth pages now, the third and fifth deleted: the first
   starts at the fourth, the second goes, the third stays, appended.
   The third and sixth deleted is a test of its own,
   `append_rewrite_labels_of_the_last_page_go_with_it`: `Stale` of
   the push button's widget, a Rewrite, the labels [(0, ""), (2,
   "A-")].
 - `a_comment_made_ours_is_saved_in_place`: `AdoptAnnotations` keeps
   each comment's id, so the plan's miscount is `Removes([Comments:
   1])`, the caret's strikeout (the replacement writes its own), not
   3; the square adopted alone appends. The asserted Rewrite and the
   removal check's veto stand; the doc comment says so.
 `append::held` over every owned key leaves
 `append_rewrite_a_reused_form_copy_appends` and
 `append_rewrite_a_page_replaced_by_its_own_page_is_a_form_of_its_own`
 appending. Each fix switched off failed its tests: the Report
 refusal skipped, the listed-removal test; the said slots empty, the
 signed-file comment, OCR and certified-veto tests; `named` off, the
 three numbering tests; `given` off, verify's; `held` back to the
 written keys, the open action's crop test; `stale`'s holders off,
 the inline link, screen, both copy's-button, duplicate's-button,
 crop link, page, field and mark tests and the 32-case property;
 the catalog walk off, the open action's and names' tests and the
 property. At 1,024 cases in release the property passes with every
 fix on (64 s), and fails with the holders off (a deleted page's
 button: `[null /Fit]` against the Rewrite's page) and with the
 catalog walk off (the open action); with `held` alone off it
 passes, as in slice 10, which checked it with both off: the crop
 tests are its check. The workspace suite: 1,166 passed, 31 ignored,
 0 failed (sheaf-doc's lib 751 and 3 ignored); clippy and fmt clean.
 The rebase's review (8 findings): an annotation the records don't
 hold that a page's `/Annots` lists by reference (a screen) stayed as
 the file had it, its `/P` the number of the page it was written
 with, when that page was renumbered (the replacement's screen
 reopened naming another page, and a planned Append wrote it so) or
 when another page is that copy now (the other file's pages
 inserted, saved, the insert undone and done again, copy 0 again:
 the screen's `/P` reached the undone page, whose widgets the form's
 check found on no page, and the save failed). Both happen only
 when what the file has of the source is copied again (`relinked`),
 and the annotation was the one object of it the emitter claims
 and so never wrote again: `emit::Cx::writes` now says a `Base` key
 of a relinked source is written, the closure's copies and these
 alike, and `Closure::stale` counts it written again with its page.
 Re-copied, not refused: it appends and reopens as a Rewrite does.
 The other file's first page has a screen now (`screen_on`), for the
 property and the fixed cases;
 `append_rewrite_a_page_appended_over_a_primary_page_writes_its_screen_again`,
 its `_wherever_it_comes` and `append_rewrite_a_reused_form_copy_appends`
 fail without the fix. The widget-walk tests and the labels' end case
 are above; two seeds from the fault runs went (Found); the module
 and design docs' merged Refused text reworded and rewrapped. After
 it: the workspace suite 1,175 passed, 31 ignored, 0 failed
 (sheaf-doc's lib 760 and 3 ignored); every corpus count as above;
 the 1,024-case property twice in release (65 s each).

   Checked by tests:
   - **A serde round trip and the schema snapshot:** sheaf-edit's
 `every_op_and_every_reference_form_round_trips`,
 `the_fixture_has_every_op_and_every_kind_of_mark`,
 `references_read_in_their_forms_and_say_what_is_wrong`,
 `patches_keep_clear_and_set`, `a_misspelled_field_is_refused`,
 `envelopes_refuse_versions_they_cant_read`,
 `range_sets_parse_sorted_and_merged`,
 `blob_ids_are_written_as_b_hex_and_length`; `the_schema_only_grows`
 against `schema/ops-1.0.json` and
 `growing_passes_and_narrowing_fails`.
   - **One test per cascade rule** (tx/cascade_tests.rs, each asserting
 the effect and the reported `Cascade`, and failing with its rule
 left out of `settle_with`): `a_deleted_page_takes_its_annotations`,
 `a_deleted_comment_takes_its_thread`,
 `a_deleted_connector_leaves_its_partner_unpaired`,
 `a_field_goes_with_its_last_widget` (`form_fields_follow_their_pages`
 inverted), `a_deleted_page_takes_its_bookmarks`,
 `a_label_range_moves_to_the_next_page_of_its_range` and
 `a_deleted_page_leaves_the_header_s_pages`; the 8-round guard's
 unit test.
   - **A batch with locals:** `a_batch_names_what_it_makes_with_locals`
 (a bookmark `$1` with three children under `New($1)`, a connector of
 two locals, a reply to a new mark: the tree, the partners, the
 reply's parent `/NM` and `Commit.created` naming every local);
 `locals_must_be_named_once_and_of_their_kind`.
   - **Unknown ids are errors** in Move, Rotate, Delete, Duplicate and
 ClearOcrText, the state unchanged: `an_unknown_page_is_not_found`;
 `every_reference_resolves_or_is_an_error` and
 `what_cant_be_done_is_invalid` for §6.4's other checks.
   - **The new-bookmark selection regression:** the app's
 `a_new_bookmark_is_the_one_selected` (see the deviation above).
   - **`MoveBookmarks` with `with_pages`:** `a_bookmark_moves_with_its_pages`
 (`outline_move_with_pages`, ported).
   - **sheaf-edit for wasm32-wasip2, and the two trees:** ci.yml's
 "sheaf-edit stays types only" and "sheaf-util stays byte helpers",
 run here (below).
   - **A plugin update keeps `created`, `/NM` and tape rows:**
 sheaf-plugin's `a_plugin_update_keeps_what_it_cant_see` (0.3 and
 0.2), beside the app's
 `a_plugins_marks_are_its_own_and_never_name_the_user`.
   - **The properties over `Op` generators:**
 `prims_rebuild_every_version` (64 cases; 4,096 ignored),
 `a_run_applies_every_kind`, `each_kind_s_no_op_pushes_nothing`,
 `prims_rebuild_every_version_past_max_versions`, and
 `append_rewrite_reopen_alike` (32; 1,024 ignored), in release.
   - **Forms:** `a_files_pages_in_runs_join_one_form` (names `[a, a2,
 both]`, 3 fields, 4 widgets, one Combine Tx),
 `corpus_a_form_combined_with_itself_is_two_forms`,
 `a_form_inserted_into_itself_goes_under_its_files_name` (both ways),
 and the notice texts, byte for byte, read from `Commit.notices`:
 forms_merge's `an_inserted_form_joins_the_documents`,
 `a_duplicated_page_gets_fields_of_its_own`,
 `scripts_from_two_forms_are_off_and_kept_apart` (both ways) and
 forms_saves' `signed_fields_brought_in_are_kept_and_said_so`.
   - **`copy.pdf`'s bytes equal to `form.pdf`'s go under "copy":**
 `an_insert_names_its_form_by_the_op`.
   - **`[delete-pages 1, rotate-pages {"pos":2}]`,** parsed from JSON:
 `positions_resolve_against_the_state_each_op_finds`.
   - **Caps:** `caps_cover_every_op_target_and_actor` (every op of the
 fixture against every target kind and every actor, through the total
 matches) and `a_plugin_is_refused_what_needs_more_than_its_cap` (a
 plugin's `DeleteAnnotations` of another tool's comment is
 `Refused(Approval)`, its `AddMark` with `imported`
 `Refused(User)`); `a_plugin_never_learns_who_the_user_is`.
   - **`Tx::check` at 7,000 pages:** `a_check_lowers_nothing` (no ids,
 nothing put; default run) and the 7k harness's row, asserted under
 1 ms with no ids drawn.
   - **Also:** `a_run_of_one_kind_is_one_line`,
 `reads_are_noted_by_value`, `a_named_blob_resolves_by_its_name`,
 `a_patch_that_changes_nothing_changes_nothing`,
 `a_format_that_isnt_one_writes_the_default`,
 `a_canceled_op_leaves_the_transaction_as_it_was`,
 `a_transaction_goes_to_another_thread`,
 `a_duplicate_s_threads_and_pairs_are_its_own` (bug 12); step 2's
 `a_form_instance_keeps_its_objects_across_saves`,
 `append_rewrite_a_form_instance_keeps_its_copy_across_saves`,
 `append_rewrite_a_reused_form_copy_appends` (insert, save, undo,
 insert, save: the reused copy's Append reopens as a Rewrite does),
 `inserting_the_same_file_twice_gives_one_source_and_two_forms`,
 `a_scripted_page_s_duplicate_turns_scripts_off` and
 `each_insert_of_one_file_is_an_instance_of_its_own`; the Op JSON fuzz
 body's `fuzz_bodies_survive_mutated_fixtures`; and the ledger's
 `nothing_retired_is_back` with slice 11's rows.

   For Greg:
   - **The Mark record isn't restructured** (§3.6's `reply_to` and
 `partner` by id, host fields out of the kind): the public surface,
 `NewMark`/`MarkKind`/`MarkPatch` and the ops, is final, so the
 record's change is internal. Recommended as a follow-up before
 slice 13 reads marks for render keys.
   - **What plugins see change:** a plugin's marks are authored by its id
 and `<<name>>` and `<<initials>>` stay as written (the date and time
 are filled); an update can't change a mark's author or stems (a
 stem it sends is ignored); a connector's ends read upright in their
 page's rotation now; a batch is one transaction of the plugin's, so
 anything beyond `Cap::Plugin` (another tool's comment deleted, which
 the 0.2/0.3 API never lists) is refused; the History panel says
 "Plugin <name>".
   - **What users see change:** a new mark's dates are the host's
 (imports keep theirs); Make Editable keeps the comment's id and its
 place in `/Annots`; an insert's marks and outline come from the
 file; unknown ids are errors; deleting a page deletes its fields (a
 field with no widget left), moves a label range starting there to
 its next page (246 of the 2,901 first-page-delete corpus outputs'
 bytes change, all with labels; no count moves) and takes it from
 the header's pages; replacing a page takes the replies to its other
 tools' comments, sheaf's reply marks included; a scripted page's
 duplicate turns scripts off; inserting a file twice gives two forms
 renamed apart; "Delete comment" deletes every same-named comment of
 another tool's on the page; status replies say "<<name>>", which the
 host fills; the removal notice adds form fields and replies;
 bookmarks a commit makes are revealed for every commit; a comment
 import leaves out (and counts) replies to nothing on their page, and
 matches another tool's comment only on its own page; Combine's marks
 come in with their pages (no adoption of a same-named comment), and
 a reply to nothing no longer fails it.
   - **The deferred ops** (`SetPageBoxes`, the watermark ops,
 `include_authored`, `FlattenAnnotations`, the blank and image page
 sources, `Transform`/`ExportTx`) come with the slices that lower
 them, rather than shipping in the schema unlowerable.
   - **Unprobed, from the rebase's verifier:** where a page isn't
 written again, `deps::Closure::stale` walks an `/Annots` entry no
 record holds with `ANNOT_OWN`, which leaves out `/Parent`, `/IRT`
 and `/Popup`, while the emitter copies those keys. A stale page
 reference reached only through such an entry's pop-up or parent
 wouldn't be seen. Not widened here, as that could move the corpus's
 refusal counts; it wants a probe of its own (Follow-ups).

   Left for later slices:
   - **Slice 12:** stale bases (`Base`, `Stale::{Validate, Relower}`)
 validated and re-lowered with the reads the Tx records (`seen`,
 `decided`), `Tx::preview`/`Preview`, ChangeSets (`Commit.change`),
 groups (`EditError::GroupEnded`, `Conflict`), `CommitOpts.{base,
 stale, group}`, the single funnel (`DocView::apply`/`apply_batch`/
 `submit`/`finish_edit` go, OCR and imports through
 `DocView::commit`), `reveal_new_bookmarks`' last form, the job API
 and the transitional job-amend path.
   - **Slice 13:** render keys and `Tx::read_render`; the realizer reads
 marks and `StepEffects.pages` for what to redraw; foreign comments
 hidden by `(blob, page, slot)` hide on every copy of that page,
 now also re-inserts of one file.
   - **Slice 15:** `SetPageBoxes`, crop as geometry; `JobKind::Trim`.
   - **Slices 16–17:** `SetWatermark`/`RemoveWatermark`,
 `include_authored`, `EditError::Content` (an op lowering content),
 the generators' scope and frozen Bates map, `HeaderFooterSpec`'s
 font; the internal `SetPageBase` and the derived table go (17).
   - **Slice 19:** `Session`, `sheaf edit` (the envelope's first
 producer, `BlobSel::With`), `ExportTx` and the `Transform`s,
 `FlattenAnnotations`, `PageSource::{Blank, Images}`, Combine without
 its write and reopen, the generated cache's stem arm.
   - **Slice 20:** WIT 0.4 crossing Op JSON, sheaf-plugin-api depending
 on and re-exporting sheaf-edit, `mark_handed_out`, and the 0.2/0.3
 adapter's deletion.
   - **Follow-ups:** the Mark record's restructure (For Greg); `/DR`
 renames at lowering; the emitter's field rule folded into
 `widget.field`; a probe of `Closure::stale`'s walk of an unrecorded
 entry's `/Parent`, `/IRT` and `/Popup` (For Greg).
