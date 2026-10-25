# M10.5 slice 20: Plugin API 0.4

Status: done · Part of [M10.5: the edit API](README.md)

**Plugin API 0.4** (L; proposals 11, 18, 19 and 20; bugs 27 and
30).
- **The WIT:** `sheaf:plugin@0.4.0` as the only world. It has
  `edits` with the envelope, `assets.upload`, and `document` reads
  in PageSpace with view matrices. Annotations exclude widgets,
  authors are roles, and text excludes fields.
- **Reads** record decision reads (0.2's and 0.3's from slice 12,
  into `CallOutput.reads`).
- **The call's Tx:**
  - built lazily from the cell (the cell and the base loaded at a
    call's first read or submit are slice 12's);
  - an end-of-call `Relower` with `created` equality (slice 12
    commits 0.2/0.3 calls' first batch with `Relower`, ids pinned,
    without the equality: nothing was handed out);
  - `commit()` mid-call;
  - truncate-or-revert rollback (slice 12's `rollback_group`, which
    0.2/0.3 calls use).
- **Events:**
  - semantic scopes;
  - merged coalescing;
  - edits triggered by an undo refused;
  - causal depth.
- **Caps** per resolved target.
- **The manifest:** `edit-schema` and `permissions` (`identity`).
- **sheaf-plugin-api** gets `Op`, the `Ops` builder and the view
  helpers.
- **The bundled plugins** are ported and bumped.
- **Docs:**
  - `docs/plugins.md`, with the generated schema;
  - the plan-file edits (M10 slices 6+ on the new API, below);
  - `plan/m11-security/README.md`, `agent.md` and `file-contention.md` updated
    (Changes to other plans);
  - the final `docs/document-model.md` pass.
- **A grep test** finds no lopdf `ObjectId` in sheaf-doc outside
  `source/`, `materialize/`, `serialize/`, `content/` and the forms
  facts.
- **Deletes:**
  - `wit/0.2`, `api/v02.rs` and `api/v03.rs` (included at
    api/mod.rs:38; 0.4's host code replaces it);
  - the double bindgen and `include!(shared.rs)`;
  - `World` and per-plugin world selection, and `API_PREVIOUS`;
  - the 0.2 fixture wasm and its tests;
  - the 0.3 edit variant and the transitional adapter;
  - `edit_from_wit` and `EditBatch`/`CallOutput.batches`;
  - docview.rs's `edit_source` (set by plugins.rs; deleted in slice
    12, a plugin's own edits told by `ChangeSet.actor`) and the apply
    loop per batch (slice 12's `plugins::land`, a call's batches in
    one `PluginCall` group);
  - sheaf-plugin-api's `Batch`;
  - `DocContext`'s per-call save `Snapshot` (deleted in slice 12:
    `DocContext.cell`).
- **Checks:**
  - **The bundled plugins** pass their tests on 0.4.
  - **Submits:** one submit adds a mark and a reply via a local, and
    a second submit references its id. An envelope with a `label`
    is `Invalid`.
  - **Rollback:**
    - a trap after two submits leaves no version and no redo entry;
    - a trap after an interleaved user edit reverts only the
      plugin's versions.
  - **Staleness:**
    - the subtotal scenario (tapes on page 3 read, the user edits a
      tape, the plugin adds a total on page 5) conflicts;
    - a user edit to an unrelated page during a call is re-lowered,
      and both land.
  - **Caps:** `InsertPages` and deleting another tool's comment are
    `Refused`.
  - **Privacy:** the plugin never receives the user's name (authored
    marks report a role, `<<name>>` stays literal).
  - **Events:**
    - a plugin isn't sent its own changes;
    - depth-2 echoes stop;
    - an undo-fighting fixture's edits after an undo are refused
      with `edit-refused`;
    - coalesced events carry the union of pages.
  - **Versions:**
    - amount-crossref refuses cached ids after a reopen;
    - a plugin declaring a newer `edit-schema` minor is refused at
      load.
  - **Assets:** an uploaded asset becomes a page-stamp mark.
  - **Build:** exactly one bindgen invocation remains.

## Notes

Done. Plugin API 0.4 (proposals 11, 18, 19 and 20; bugs 27 and 30):
   `sheaf:plugin@0.4.0` is the only plugin world. A plugin reads the
   document in PageSpace, each `page-info` carrying its `to-view` and
   `from-view` matrices, and edits with sheaf's own ops: Op JSON in the
   envelope, lowered at once into the call's one transaction, built
   lazily from the published cell (`Published::plugin_tx`), answered
   with what it made (`Applied`), previewed with its caps, committed
   mid-call through the UI thread (`commit()`), and landed at the call's
   end with `Stale::Relower` and `created` equality (`Tx::
   mark_handed_out`), or rolled back (truncate or revert). Its reads are
   the transaction's decision reads and see its own writes; authors are
   roles (`you`, `plugin <id>`, `other`), and the user's name is told
   only with the new `identity` permission; it uploads what its edits
   hold (`assets.upload`) and never names a blob. Events are
   `changed(change-info)` with semantic scopes, merged per plugin and
   document while it's busy (those alike to answer), never sent to the plugin that made them,
   and an answer is refused past depth 1, to an undo, or to
   `edit-refused`, which goes to subscribers. WIT 0.2 and 0.3, their
   adapter, the 0.2 fixture and sheaf-plugin-api's `Batch` are gone with
   no shim; the six bundled plugins are ported. PDFs made in memory have
   one permanent builder (`serialize/built.rs`); exchange's comments
   and the summary's page forms are read as sources; and model-level
   code names no lopdf `ObjectId`, which a `syn` test holds. The docs'
   final pass: docs/plugins.md for 0.4 with a tested examples block,
   README, m10.md's slices 6+ on the new API, m11.md, agent.md,
   file-contention.md and plan/complete/m06-plugins.md, and docs/document-model.md's
   slice-20 and Invariants sections. Every asserted gate holds, and the
   corpus counts are slice 19's.
   - **Groundwork** (step 1). sheaf-edit: `reply.rs` (new) with `Made`
 (moved from sheaf-doc), `Applied { created, locals, lines }`,
 `Previewed { lines, created, locals, caps, removes }`, `Failure {
 kind, message }` and `FailureKind` (mirroring `EditError`), serde and
 JsonSchema, `deny_unknown_fields`; `Cap` moved here with its Display
 words; `envelope::reads` public. ops-1.0.json byte-identical; builds
 for wasm32-wasip2. sheaf-doc: `tx::Made` and `tx::Cap` re-exports;
 `Tx.handed_out` and `Tx::mark_handed_out` (through `TxParts` and
 `relowered`), `Document::relower` comparing the original `created`
 (ops' ids and locals) with the new when handed out: a Conflict over
 the first lowering read whose value changed (`by` from
 `self.conflict`), else over the first slot the new steps wrote or the
 first thing made at first (`by: None`); `CommitOpts.depth` and
 `ChangeSet.depth` (commit, amend and an empty commit carry it, every
 other cause 0); `Published { snap, lowering }`,
 `Document::published()`, `Published::plugin_tx(id, version,
 identity)` (`Actor::Plugin` alone, `HostAttrs::now` at the call),
 a private `Lowering` (store, allocator, identity, file name, ready)
 that `lower_cx` builds through with `identity: false`;
 `LowerCx.identity` (tx/marks.rs' `filled`: `<<name>>` and
 `<<initials>>` only with the grant, `<<date>>` and `<<time>>`
 always); `Tx::apply_said` (that batch's steps, ops, locals and caps)
 and `Tx::preview_batch` (checkpoint, apply, rewind), `Preview.caps`
 from `Entry.cap`, `allowed()` returning the cap; `From<Preview>` for
 `Applied` and `Previewed`, `From<&EditError>` for `Failure`;
 `ViewInputs::print(false)` pinned equal to `plugin()` with marks and
 others' comments off. sheaf-plugin: `Manifest.edit_schema`
 (validated: "built for edits 1.1; this sheaf reads up to 1.0"),
 `Permissions.identity` and `Grants.identity` ("Know your name");
 change.rs (new): `ChangeInfo { version, parent, instance, cause,
 actor_kind, scopes, depth, undone }`, `ChangeCause` (with
 `Coalesced`), `ActorKind` (with `Mixed`), `Scopes` (with
 `generators`), `ChangeInfo::of` and `merge`.
   - **WIT 0.4, the host, the SDK and the fixture** (step 2).
 `wit/sheaf.wit` rewritten as 0.4 (`affine`, `version`, `page-info`
 with label and matrices, a read-only `annot` whose author is a role,
 `comment-summary`, `annot-view`, `change-info` and `scopes`,
 `doc-event` with `changed` and `edit-refused`; `document.version`;
 `render-options.without-markup`; PageSpace OCR words; `edits.submit
 (label, ops-json)`, `preview`, `commit() -> result<version,
 string>`; `assets.upload(bytes, asset-kind) -> asset`), every
 function documented with its reads, frame and privacy; `wit/0.2`
 deleted. sheaf-plugin: one `bindgen!` (`additional_derives:
 [PartialEq]`); the host split into `api/{convert, document, text,
 render, edits, assets, ui, progress, storage}.rs`; `API_VERSION`
 0.4.0 and the exact-minor check; `HostState::tx` (the call's lazy
 transaction on `ArcSwap<Published>`, its uploads pinned and granted
 again in each), `drawn` (a page read through `read_render` on the
 transaction's own state), `Roles`; landing.rs (new): `Door` (and
 `impl Door for Document`), `Committer`, `Committed`, `Landing`
 (`commit`, `finish`, `end_group`), `Landed` and `follow_ups`;
 `DocEvent::{Changed, EditRefused}`; `RenderSource::render(&
 ViewInputs)` drawing the page as it shows; `stamp_lib::
 image_stamp_bytes`; `MAX_UPLOAD_BYTES` (64 MiB a call). sheaf-doc:
 `Tx::{base, is_empty, decided}` (`decided` for tests and fixtures),
 `Published::{store, is_user}`, `Document::instance`.
 sheaf-plugin-api 0.4.0: depends on and re-exports `sheaf_edit`;
 ops.rs (new: `Ops` and `commit`, `upload`, `style`, `page`, `at`,
 `BLUE`) and view.rs (new: the matrices' helpers); batch.rs deleted.
 The fixture on `api = "^0.4"`, `edit-schema = "1.0"` and the new
 events, in PageSpace through `view`. The app: `plugin_cell` an
 `ArcSwap<Published>`, republished at every change and in
 `sync_author`; `EngineRender` drawing as the page shows
 (`render_delta`); `run` landing through `Landing::finish`
 (`land_call`); `plugin_event` mapping a change to `Changed(
 ChangeInfo::of(..))`, saves and opens left out. tests/one_world.rs
 (new); host.rs ported onto a `Shared` harness (the document and its
 `Landing` behind a mutex, the cell, `impl Committer`).
   - **The bundled plugins and the fixture's checks** (step 3). All six
 at 0.3.0 with `api = "^0.4"` (`edit-schema = "1.0"` but subtotal,
 which edits nothing), their Cargo.lock files updated offline:
 tax-form-bookmarks appends (`Ops::bookmark`, `Sibling::Last`, no
 reverse fold); bookmark-from-pattern names a new heading by a local
 and lands 12 points above a match as the page shows
 (`view::dest_from_view`, new), `last_children` gone; amount-crossref
 places its square beside an amount as the page shows
 (`connector_rect`) and keys `PENDING` by path and
 `version.instance`, refusing another instance ("the document was
 opened again; find again"), as duplicate-pages does; subtotal
 subscribes to `changed` and forgets by scopes; currency-convert
 builds with `Ops::mark`. Five READMEs updated. The fixture's 13 new
 commands (thread, labelled, two-then-trap, commit-then-trap,
 commit-wait-trap, subtotal, insert-pages, delete-comment, flatten,
 raw-blob, authors, stamp-upload, read-back), a 73-byte PNG and a
 hand-written one-page PDF; host.rs's `bundled()`, `opened()`,
 `commented()`, `taped()`, `crossref_answers()` and
 `FakeUi::{meanwhile, editing, told}`.
   - **Events and the app** (step 4). sheaf-plugin: `runtime::answering(
 call) -> (depth, refusal)`; an answer to `edit-refused` refused too
 and not told again (Deviations); a refused call's `preview` and
 `commit` refused as its `submit`; the fixture's answer modes
 (storage key "answer": mark, commit, insist). The app:
 `AppCommitter` sending `CommitRequest { tx, label, depth, reply }`,
 handled in `run`'s `select_biased!` loop on the UI thread
 (`CommitRequest::land`, `DocView::commit_call`, only while the view
 shows the call's cell), the call's `Landing` a local of that task
 taken by `land_call`; a dropped channel or reply `EditError::
 Invalid { op: "commit", why: "its document was closed" }`;
 `plugin_context(ocr, committer)`; `follow_up_calls(manifest,
 &landed)`; `Latest<T>` became `Coalescer<K, T>` with a fold,
 `PluginHost.regions` (latest per plugin) and `PluginHost.changes`
 (merged per plugin and view), `send_now`, `next_from` and
 `queue_done` (for `region_done`); `recipients(plugins, kind, from)`.
   - **PDFs made in memory** (step 5). serialize/built.rs (new):
 `ResObjects` (from content/to_lopdf.rs, deleted; `bind` through
 `content::sink::bind`), `WriteCx`, `StampForm` (with `resource()`)
 and `Held` from annot.rs, the fixtures `Written`, `to_pdf`,
 `to_pdf_in` and `write`, a private `Graph` sink (for `sink::
 Lopdf`), `as_form` (for `Appearance::write`), and `Built` (`new`,
 `From<lopdf::Document>`, `pdf`, `page`, `form_page`, `finish` by
 `write_built`). annot.rs: `written_staleness` (the digest from
 direct values; `staleness` over a shared `digest`; `NoObjects`
 gone), `tape_pdf` built with `Built` and written by `write_built`,
 `pinned_pdf` through `WriteCx::stamp` and `Built::form_page`,
 `WriteCx::stamp` reading each frame from its source, appearance or
 `page_form`. Exchange: `Comments { src, annots, file }`,
 `Comments::built`, `MemoryKind::Comments` (no pages allowed, as
 Blank), `BlobKind::comments(password)`, `from_pdf` lazy with its
 password, `fdf::write` copying each comment out. stamp_lib:
 `page_forms(&Src, &[u32], dst)` (content as one stream, own or
 inherited resources by `remap`, the matrix as written, -0 made 0),
 `page_form(&Src)`, `form_to_pdf` through `Built`, `copy_page_dict`
 moved in from `materialize::copy`; `pub(crate) Blob::source(&Limits)`
 (`Store::source` delegates); the emitter's `blob_form(&Blob, index)`.
 The summary opens its printed bytes as a Summary source and builds
 with `Built`.
   - **No lopdf ids in the model** (step 6). `source::PageObj { blob, id
 }` and `Source::page_obj`, keying `page_showing`, `page_named`,
 `pages_showing` and the save's maps (`emit::written_pages`,
 `copy::Pages::Written`, `copy::page_named`, deps.rs' `Written`);
 `FlatOutlineItem` to open/outline.rs (still `sheaf_doc::
 FlatOutlineItem`); `holding` and `file_in` taking `ObjRef`s;
 source/appearance.rs (new: `Shows::{read, in_slot, form}`, `frame`,
 `unkept`, from tx/flatten.rs, which keeps `placed`);
 generate/saved.rs (the saved-generator reader, moved as it was,
 re-exported); kept.rs to `materialize::kept`. tests/object_ids.rs
 (new, `syn`): the walk from lib.rs by `mod` declarations, test-only
 and fixture code left out, paths, types, `use`s (renamed too) and
 macro tokens; four tests (no hits; every file walked or built only
 for tests; the allowlist's entries each still naming `ObjectId`,
 with a reason, none covering model.rs, model/, tx/ or annot.rs; a
 probe crate in memory).
   - **Docs and plans** (step 7). docs/plugins.md rewritten for 0.4 (the
 quick start on `Ops`; the manifest's `edit-schema`, `identity` and
 events; PageSpace and the maps; reads as decision reads; marks,
 summaries and roles; text, renders and OCR; one transaction per
 call; "What a plugin may do"; assets; events and answers; limits;
 the edits schema with three `<!-- plugin-ops example -->` blocks;
 versioning as one world; trust; testing with `Shared`) and
 sheaf-plugin's tests/docs_examples.rs (new, `EXAMPLES = 3`); README
 (plugins on 0.4, the Save model row, the recovery text, and in
 review the Document edits row: ops lowered into the model and
 sheaf's own emitter, lopdf the object types and what's loaded
 whole, where it still said edits are made on the raw object graph);
 plan/complete/m06-plugins.md's N-1 lines superseded; three present-tense 0.3
 statements in document-model.md made history; m10.md, m11.md,
 agent.md and file-contention.md (For Greg, and Deviations).
   - **Wrap-up** (step 8). gate_7k: the snapshot row's `Document::
 published()` beside it, under the same `SNAPSHOT_GATE`, asserted.
 retired.rs: 55 slice-20 rows in seven groups with their comments
 (step 8 added `api::shared::{pt_from_wit, rgb_from_wit}`);
 `QUALIFIED_ONLY` keeps "Edit" (`history_panel::Row::Edit`, `sheaf
 edit`'s `Command::Edit`), "NewOutlineItem" gone, and the entries the
 steps added each with its reason. The ledgers (above). docs/
 document-model.md: "Invariants, and the tests that hold them" after
 the core idea (§2's twelve rules, the registry rule, nothing
 unproduced and the private-data table, each with its tests), "Plugin
 API 0.4 (M10.5 slice 20)" after slice 19's, its forward references
 made history, and bug 30's last claims (the "lopdf (writing)"
 framing, lopdf as "a serialization backend") rewritten; the `Vec`
 history, `usize` cursor, `Arc<Vec<PageRef>>` and "lopdf doesn't
 write incremental updates" were gone since slices 1, 8 and 10.
   - **Deleted,** each confirmed dead by `cargo check --workspace
 --all-targets` and clippy, the names in retired.rs: `wit/0.2`;
 `api/v02.rs`, `api/v03.rs` and `api/shared.rs` (`Adapter`, `Said`,
 `More`, `page_frame`, `to_frame`, `dest_from_wit`,
 `rotation_from_wit`, `pt_from_wit`, `rgb_from_wit`), the two
 bindings (`bindings_v02`, `bindings_v03`) and `include!`;
 `API_PREVIOUS`; `runtime::{World, EditBatch, Held}`,
 `CallOutput::{batches, reads, base}`, `HostState::{batches, reads,
 read, page_base}`; the 0.2 fixture (`tests/fixtures/test-plugin-0.2`,
 164,420 bytes of wasm) and its tests (`load_v02`,
 `a_plugin_built_for_02_still_loads_and_runs`,
 `api_02_s_page_text_layer_answers_none`); 0.2's
 `ocr.page-text-layer`; `DocEvent::Edited` and `EventKind::Edited`;
 the app's `plugins::{land, Landed, edited_label, Latest,
 region_done}` and `edited_label`'s test; sheaf-plugin-api's
 `batch.rs` (`Batch`, `annot`, `at`, `BLUE`); `content::to_lopdf`,
 `content::ResObjects`, `content::sink::Lopdf`, `annot::{WriteCx,
 StampForm, Held, Written, to_pdf, to_pdf_in, NoObjects,
 Appearance::write}` (moved or replaced in `serialize::built`);
 `materialize::copy::copy_page_dict` (moved);
 `exchange::Comments::{pdf, new}`; lopdf's `save_to` in
 `annot::tape_pdf`; `model::FlatOutlineItem` and
 `tx::flatten::{Shows, read, shown, frame, unkept}` (moved).
 Verified gone from earlier slices: `edit_from_wit`, `sign_marks`
 and `keep_unseen` (slice 11, retired.rs), docview's `edit_source`
 and `DocContext`'s per-call `Snapshot` (slice 12). `rg 'save_to\('
 crates/sheaf-doc/src` finds only tests and fixtures.

   Deviations from the plan (the planner's list first, as resolved):
   - **The `ObjectId` grep has an allowlist** (§2 rule 7's test): as
 worded ("outside `source/`, `materialize/`, `serialize/`,
 `content/` and the forms facts") it couldn't pass, since 30 files
 outside those name `ObjectId`. The model-level ones were cleaned
 (model.rs, model/state.rs, tx/pages.rs, tx/flatten.rs, generate.rs,
 annot.rs); the file-level readers that read a file's objects by
 number are allowlisted by file, each with a reason: open/annots.rs,
 open/outline.rs, attachments.rs, labels.rs, layers.rs,
 portfolio.rs, properties.rs, file.rs, import.rs; realize/alloc.rs,
 assemble.rs and untouched.rs; private.rs, stamp_lib.rs,
 exchange/mod.rs and generate/saved.rs. Directories are named only
 where the plan named them, so open/mod.rs, realize/mod.rs,
 realize/inputs.rs, realize/key.rs, exchange/fdf.rs and
 exchange/xfdf.rs stay checked. The alternative, moving the fact
 readers under source/, is a large churn of public paths (For Greg).
   - **No clipboard or export function in 0.4:** §14.3 has none, and
 D46 keeps exports out of plugins' reach, so slice 19's "WIT 0.4's
 clipboard" is `assets.upload` alone, and "plugins' flatten and
 exports" is a plugin's `FlattenAnnotations` refused as
 `Refused(Approval)` (tested) and `Transform` outside the envelope.
 `actor_exports`' plugin arm has no WIT producer (For Greg).
   - **Uploads are interned like a paste's assets** (`Published::store`,
 `BlobKind::stamp()` or `Attachment`), granted in the call's
 transaction, pinned, and granted again in order in each transaction
 a `commit()` starts, so a handle outlives a mid-call commit. An
 image becomes a one-page stamp PDF (`image_stamp_bytes`); a `pdf`
 upload is a stamp's page (`Grant::Stamp`), so an attachment or an
 insert's file is a `file` upload. The 64 MiB cap counts the bytes the
 plugin handed over and, for an image, its pixels at four bytes each,
 told by its header before it's decoded, the decoder held to them
 (review: decoding runs on the host, where the call's deadline can't
 stop it).
   - **A plugin's marks are authored by its id,** not its name as §6.8
 says: the id is stable, is what History shows, and matches the
 `plugin <id>` role. Under `identity`, authors are told by name but an
 installed plugin's, still `plugin <id>`.
   - **`edit-schema` is optional:** a plugin that only reads (subtotal)
 declares none, and one that leaves it out has every `submit`,
 `preview` and `commit` answered `invalid` ("the plugin's manifest
 doesn't say which edits it writes"). A newer minor than sheaf reads
 is refused when the manifest is read, so `discover` lists it as an
 error and no plugin is made: the planned host.rs load test asserts
 that (`a_plugin_built_for_newer_edits_is_refused_at_load`), not a
 running plugin's `Status::Failed`.
   - **`edit-refused` goes only to subscribers** (`follow_up_calls`), and
 edits answering it are refused ("it answers a refusal of its edits")
 and not recorded in `CallOutput.refused`, so they're never followed
 up: otherwise a plugin could fight undo in two steps (the refused
 answer to an undo told, then re-added at depth 0). For Greg.
   - **An end-of-call failure rolls back the whole call:** a last commit
 that fails (a Conflict on Relower) takes back what the call's
 mid-call commits landed too (`Landing::finish`), so a call is all or
 nothing as slice 12's batches were. A stopped call that returned
 `Ok` changes nothing ("… was stopped, so it changed nothing"); one
 whose rollback conflicts says "… was stopped; its partial edits were
 kept".
   - **Renders and regions in PageSpace, drawn as the page shows:**
 `RenderSource::render` takes the `ViewInputs` (`plugin()`, or
 `print(false)` for `without-markup`, equal to `plugin()` with marks
 and others' comments off, so no `plugin_bare()`), and draws the page
 turned as it shows (`render_delta`), its `to-page` and `region` in
 PageSpace. Text and renders read the transaction's own state, keyed
 by render key.
   - **`Made` and `Cap` moved to sheaf-edit** (re-exported at
 `sheaf_doc::tx`), so a reply is typed JSON, `Previewed.caps` naming
 caps in sheaf's words; `Applied.locals` and `Previewed.locals` are
 `BTreeMap<Local, Made>` ("$1" keys in JSON). `Previewed` carries
 `locals` too, and `removes` is the batch's `Flags::REMOVES`.
 ops-1.0.json is unchanged: none of them is in the envelope.
   - **`Published::plugin_tx`:** §14.1's cell publishing "the snapshot
 and `LowerCx`" can't bind an actor per document, so the cell holds
 `Published` (the snapshot and a private `Lowering`), whose
 `plugin_tx` binds `Actor::Plugin` alone; `lower_cx` stays the only
 way to make any other actor's transaction, and never grants
 identity. `Published` takes the primary from `snap.primary`.
   - **kept.rs and generate.rs moved:** `keep_only` is
 `materialize::kept` (no retired rows: `kept::keep_only` and
 `kept::Kept` still match their live uses); generate.rs' saved
 reader (`Saved`, `read`, `Block`, `rebind`) moved as it was to the
 allowlisted child `generate/saved.rs`, since `Saved` is cached in
 the `Source` and can't hold `ObjRef`s; generate.rs itself is
 checked. `FlatOutlineItem` moved to open/outline.rs for the same
 reason (cached in the `Source`; an `ObjRef` there would be a cycle).
   - **The page key is `PageObj { blob, id }`,** not an `ObjRef`, which
 would pin its source, cost an `Arc` clone a key and need a `Src`
 for every range bound. The save's maps took the same key so one
 `page_named` serves both; Relinking, Closure and `page_of` build it
 where they look up.
   - **The refusal rules are the host's** (`runtime::answering`, from
 step 2), since a refused `submit` must say why and the rule depends
 only on the event. The depth refusal says "it answers a plugin's
 answer to a change, and those stop there": a command's edits are
 depth 0 and may be answered; only answers to answers are refused.
   - **Changes are merged per plugin and view,** not per plugin:
 `ChangeInfo::merge` replaces what waited when the instance differs,
 so merging two documents' changes would drop one. Regions stay
 latest-wins per plugin. `Latest<T>` became `Coalescer<K, T>` with a
 fold, serving both.
   - **Only changes alike to answer merge** (review), beyond §14.3's
 merge rule, which says nothing of depth or undo: a change a plugin
 may answer (`ChangeInfo::answerable`: depth 0, not undone) merges
 only with another like it, and one it may not only with another
 like that, so a merged change is refused an answer only when each
 of its parts would be. As first built, the merge took the deeper
 depth and OR-ed `undone`, so a user's edit made while a plugin was
 busy, after another plugin's answer or an undo, merged into a change
 whose answer was refused, and was never answered. Neither the
 shallower depth nor the latest `undone` would do: both let an answer
 land on an undo or on another plugin's answer merged with it. So a
 view's waiting changes are a short queue (`Coalescer` holds a
 `VecDeque` a key), the later change folded into the last when alike
 and queued after it when not; a change to a document a reopen made
 drops those waiting for the old one.
   - **The planned bookmark-span test** (`DuplicatePages { pages:
 Bookmark(b) }` with a page added under b) can't work as written:
 bookmark resolution reads `Key::Order`, so any order change
 conflicts, handed out or not. The test moves the next bookmark from
 page 4 to 5 instead (the order alone kept): without
 `mark_handed_out` it lands with 3 copies, with it it's `Conflict {
 by: None }` (Found).
   - **`Scopes.generators`,** beyond §14.3's sketch: a watermark or
 header change writes `Key::Generator` alone, which `ChangeSet.content`
 doesn't list, so plugins would otherwise get no scope for it and
 their text and render caches would go stale. `content` adds pages
 added and removed; `form` is `form` or any field; `undone` is Undo,
 Redo, Jump, Rollback or Revert; `ActorKind` is User for the user,
 the command line and any later actor (`Actor` is non_exhaustive).
   - **subtotal forgets by the text's scopes,** not `scopes.annots` as the
 plan says: 0.4's text leaves annotations out, so a change to them
 never changes the words it sums, and keying by annots would re-read
 for nothing and miss what does change them (content, OCR, headers,
 watermarks, and page numbers through order or labels). The plan's
 tape scenario is the fixture's `subtotal` command, as specified.
 amount-crossref and duplicate-pages key `PENDING` by path and
 instance, so a different path still has nothing waiting.
   - **`document.version` is `option<version>`** (none with no document),
 imported as `version as doc-version` since WIT refuses a type and a
 function of one name in an interface. `pages()` also reads
 `Key::Labels`, as `page-info` carries labels; `annotations()` leaves
 out what the file hides as well as links and widgets.
   - **The 0.3 fixture's `place` bookmark** (landing at the shown top
 only) went: a plugin writes `/XYZ` in PageSpace, or maps one axis
 with the new `view::dest_from_view`, which bookmark-from-pattern
 needs on turned pages.
   - **Landing's tests moved into sheaf-plugin** (landing.rs) with
 `Door`: `a_call_s_commits_are_one_undo_step`,
 `a_failed_or_stopped_call_takes_back_what_it_committed`,
 `EditedBetween`, the plugin-marks test, and the new
 `a_call_that_left_nothing_is_told_its_refusals`; the app keeps the
 GPUI landing tests and `a_plugin_s_own_edit_names_it`. Host tests
 renamed for 0.4 (`a_call_s_submits_are_one_undo_step`,
 `text_comes_as_the_file_draws_it_in_page_space`,
 `every_kind_is_shown_and_the_preferences`,
 `a_plugin_places_through_the_page_s_view_matrices`).
   - **`CallOutput.tx` is None** when the call applied no op, so a call
 that only read commits nothing.
   - **`insert-pages`' refusal uploads a `file`:** a `pdf` upload is a
 stamp's page, which InsertPages rejects as Invalid before the cap
 check. The refusal commands pass sheaf's JSON answer back through a
 notice and return Ok, so a test asserts the exact `Failure` and that
 nothing was lowered. "Draws" for an uploaded stamp is checked in the
 saved file (each stamp's `/AP /N` content holds ` Do`), with no
 renderer in sheaf-plugin.
   - **duplicate-pages puts a group back, and amount-crossref keeps its
 pairs,** when their submit fails: 0.3's `Batch::submit` couldn't.
   - **The `Objects` trait and its eager arm stay** for what still loads a
 file whole (a Rewrite's graph, F15; `forms::check`; the stamp
 library, proposal 29; tests); its doc says so (For Greg).
 `form_to_pdf` stays generic over `Objects`, every lib caller passing
 a `LazyPdf`: making it concrete cascades into `import::convert` and
 `file::annots_in`, whose tests pass loaded documents.
   - **`Comments` is one type both ways:** `gather` builds into a `Built`
 and makes it a source too, so `fdf::write` copies each comment out
 (`copy_object`): FDF bytes change (renumbered, only what the
 comments reach) and read back alike (the same XFDF and marks); XFDF
 output is byte-identical. `page_forms` copies a page's resources and
 content alone, where the old code copied its other keys (Metadata,
 Group …) into the output as orphans.
   - **`WriteCx::stamp` reads a copied form's frame from its source**
 before copying, so a Base form with no box is no longer copied in
 before it's refused; `tape_pdf` and `pinned_pdf` stay in annot.rs
 (they draw content and name no `ObjectId`), so stamp_preview.rs is
 unchanged.
   - **QUALIFIED_ONLY grew** for names that live on elsewhere (World,
 Held, Said, Landed, land, to_frame, reads, at, BLUE, annot,
 without_markup, plugin_tx, ResObjects, WriteCx, StampForm, Written,
 to_pdf, to_pdf_in, new, FlatOutlineItem, Shows, unkept), each with
 its reason; the `api::shared` module has no row of its own
 ("shared" is common), its items do.
   - **m10.md, m11.md, agent.md and file-contention.md** (step 7):
 m10.md slice 22's `PageSource::Template` arrives with `spawn`, not
 before (M10's proposal 13 and Not in M10 put spawning out of reach);
 slices 7 and 8 write the `SetFieldValues` rules standalone for slice
 9's op; slice 29's plugin field ops are setting and resetting
 values, a flatten needing approval; a few checks added (slices 6, 7,
 9, 10, 11, 12, 14, 20, 25 and 29) and slice 9's permissions-only
 notice check replaced by "encryption kept both ways and through
 recovery"; stale references to replaced code fixed in proposals 8
 and 10. agent.md says decision reads are kept "per agent session"
 (its Transport section has no MCP session); file-contention.md's §1
 gains what M10.5 did. m11.md's slice 1 is renamed "Encrypted files:
 what's left" and keeps bug 12. README's Save model row leaves out "a
 field's value" (no fills until M10 slice 9), and its "saves write a
 plain file" line was already gone.
   - **The docs examples name pages 0 to 3** (ids, as a plugin names
 pages), asserted on a blank four-page document. docs/plugins.md
 says a plugin's `preview` caps are "plugin", since an op it may not
 make fails as `submit` would.
   - **The Windows check of sheaf-plugin can't run here** (wasmtime's
 fiber and ring need windows.h and MSVC), as before; sheaf-plugin's
 only platform cfg is a `cfg(unix)` in a test.

   Found while building:
   - **`PageSel::Bookmark` records only `Key::Order`,** not the outline
 `bookmark_span` reads, so a `Relower` after another bookmark moved
 re-resolves to a different span without a conflict (only a
 handed-out transaction conflicts, and its Conflict names no edit).
 Decision tracking of bookmark selections has the same gap. Backlog
 B19.
   - **A region call in flight when its window closes** leaves the
 plugin's `regions` key busy (`ws.update_in` fails), so its later
 region events in any window wait forever. Before this slice; changes
 are keyed per view and aren't affected. Backlog B20.
   - **`layers::LayerId` is `pub type LayerId = ObjectId`,** used by
 `ViewInputs::off` and the app's layers panel: a layer is named by its
 file object id outside the allowlist, which the grep can't see
 through an alias. Backlog B21.
   - **`FlatOutlineItem` derives Serialize and Deserialize** with no
 serializer in the workspace (before this slice). Backlog B22.
   - **Add from PDF Page… loads the user's whole PDF and clones it**
 (`stamp_lib::one_page` via `file::load_pdf`, `page_without_extras`)
 to copy one page: slice 19's Found (it drops the page's `/Annots`,
 widgets included, and reads the file, not the unsaved state), both
 carried into m10.md's slice 12.
   - **No test covered comment import from another PDF** before this
 slice: `another_pdf_s_comments_are_read_from_it` (plain and AES-128
 with a password) is new.
   - **qpdf 12.3.2 can't check a zero-page PDF** (its own vector
 range-check error, on a hand-made one too), so the comments' memory
 source isn't qpdf-checked; an FDF has no page tree either, before or
 after.

   What review found and fixed: the plan was reviewed before the steps
   started, and its findings are the deviations above, as resolved
   (the plan-versus-code list: deletes already done, `api::shared` as
   the adapter, caps already per target, identity half done, the cell
   and `LowerCx`, the grep as worded, what slice 19 left, no clipboard
   or export WIT, uploads across a commit, `QUALIFIED_ONLY`, agent.md's
   and README's moved lines, docs/forms.md not existing yet, reads from
   the transaction, the schema unchanged, `document.path`). Each step
   checked its new checks by switching their behavior off (Checked by
   tests). The slice's own review (11 findings, each checked against the
   code) found, all fixed:
   - **A user's edit merged with what it may not answer** (four
 findings, one defect): `ChangeInfo::merge` took the deeper depth
 and OR-ed `undone`, so a plugin busy while another plugin answered,
 or the user undid, then edited, had its answer to that edit refused
 and never re-triggered. Only changes alike to answer merge now
 (Deviations); `a_users_edit_waits_apart_from_changes_it_may_not_
 answer` (app) and `changes_merge_only_with_those_alike_to_answer`
 (change.rs) are new. The fixes the findings proposed differently
 (the shallower depth, the latest `undone`) were not taken: each
 lets an answer land on an undo merged with a later edit.
   - **An uploaded image was decoded unbounded on the host** (major): a
 small PNG of a huge image decoded to gigabytes inside the host call,
 where neither the 256 MiB memory limit nor the deadline reaches.
 `image_stamp_bytes(bytes, budget)` reads the header first and
 refuses an image whose pixels (four bytes each) pass what's left of
 the call's 64 MiB, then decodes under `image::Limits` of those
 dimensions and that budget; the pixels count against the cap.
 `an_image_stamp_is_made_within_its_budget` is new; docs/plugins.md
 and the WIT say how an image counts.
   - **A library stamp's size and its form's disagreed** on a real
 `/Rotate` or a box number by reference: `page_forms` reads the page
 table's geometry, `stamp_lib::geom` read `/Rotate` with `as_i64`
 and boxes through `box_of`, so a page with `/Rotate 90.0` was placed
 unrotated and drawn squashed. `geom` now reads as `source::pages`
 does (an entry that resolves is the page's, read or not; numbers
 through references; a real `/Rotate` cut to a whole number);
 `a_stamp_is_sized_as_its_form_draws_it` is new and fails with the
 old `/Rotate` read.
   - **Docs:** README's Plugins paragraph said a plugin never learns
 the user's name, though `document.path` is served (narrowed, and
 docs/plugins.md's Identity bullet says so too); README's Document
 edits row described the pre-M10.5 design (rewritten, above);
 m10.md's slice 12 check and proposal 10 named `SHEAF_CACHE_DIR`,
 gone in slice 19 (restated as no new file under the temp directory
 or sheaf's state and cache folders, the formjs compiled guest
 aside).
   - **The SDK's dependency on sheaf-edit had no version** (path only),
 which `cargo publish` refuses: the workspace dependency has
 `version = "0.1.0"` now, and docs/plugins.md shows the SDK as a path
 dependency, since neither crate is on crates.io (For Greg).
   - **`HostState::refused()` shared its name with the field** it fills:
 renamed `unless_refused()`.

   Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
   - **The gates** (`gate_7k`, release, each test in a run of its own,
 each started once the 1-minute load was under 4 on two checks 30 s
 apart: load 1.5 to 3.8 at each start; 11 of 11 pass). The one new
 row, `Document::published()` (the median of 21), takes 0.361 µs on
 the synthetic, 0.391 µs on the heavy and 0.361 µs on the scanned
 (snapshot 0.271, 0.280 and 0.241 µs), asserted < 50 µs. The rest,
 against slice 19's (this slice's paths reach none of them):

 | Row | Synthetic | Heavy | Scanned |
 |---|---|---|---|
 | geometry walk (asserted < 100 and < 250 ms); ready (asserted < 1 s on the heavy) | 50.0 ms; 54.2 ms | 77.2 ms; 379 ms | 70.3 ms; 463 ms |
 | `Tx::check` of a delete of every page (asserted < 1 ms) | 0.431 ms | 0.411 ms | 0.419 ms |
 | realize + open + render 1,024 px, an edited page (asserted < 15 and < 25 ms) | 12.0 ms (slice 19: 11.8) | 21.5 ms (21.2) | 34.5 ms (31.7) |
 | engines after 220 groups: weighed, RSS added, open (asserted ≤ 96 MB, ≤ 96 MB, ≤ 6) | 28.9 MB, 41.7 MB, 6 | 28.9 MB, 65.4 MB, 6 | 29.0 MB, 61.2 MB, 6 |
 | thumbnail strip, crop + watermark + header, 320 px (asserted ≥ 310 pages/s, ≤ 96 MB weighed and added) | 425 pages/s, 80.7 MB, 25.8 MB | 352 pages/s, 80.7 MB, 60.9 MB | 164 pages/s |
 | crop all pages, and validated after a delete (asserted < 200 ms) | 14.2, 15.5 ms | 10.9, 15.7 ms | 11.9, 15.7 ms |
 | Bates apply (asserted < 50 ms) and its Rewrite's fonts (asserted 1) | 0.621 ms, 1 | 0.633 ms, 1 | 0.640 ms, 1 |
 | census of a watermark and a PNG background (asserted 1, 1, 1, 0; page resources) | 1, 1, 1, 0; 1 of 1 | 1, 1, 1, 0; 70 of 70 | 1, 1, 1, 0; 7,000 of 7,000 |
 | recognized text on every page: census (asserted 1 font, one block a page); the Append | 1, 7,000; 9.77 s | 1, 7,000; 10.08 s | 1, 7,000; 9.84 s |
 | one-mark Append on the scanned (asserted < 50 ms) | | | 30.0 ms (slice 19: 26.4) |
 | "Page n of N", saved, every page moved: the Append (asserted ≤ 1 s), the section (asserted ≤ 8 MB) | 459 ms, 1.36 MB | 747 ms, 1.47 MB | 468 ms, 1.37 MB |
 | the same move on the file opened again: the Append (asserted ≤ 1 s on the scanned; accepted misses on the others), the section (asserted ≤ 8 MB) | 893 ms, 3.31 MB | 1,574 ms, 3.33 MB | 936 ms (see below), 3.32 MB |
 | proposal 8's draws: set unsaved, a moved page, one deleted, one deleted on the file opened again (accepted misses on the synthetic and heavy) | 22.4, 20.0, 18.7, 29.2 ms | 36.5, 33.0, 33.7, 40.5 ms | 38.4, 33.4, 31.7, 41.9 ms |
 | Export Flattened: time; peak RSS (the heavy's ≤ 221 MB asserted only with `SHEAF_GATE_FLATTEN`); the open's peak before it | 372 ms; 88.2 MB; 37.9 MB | 1.31 s; 512 MB; 221 MB | 647 ms; 1,077 MB; 523 MB |

 The scanned's Append on the file opened again sits near its gate,
 as slice 19 found (886 ms, and after its review 1,067 ms once, then
 940, 887 and 934): it tripped at 1,065 ms on the first run (load
 3.7 at the start, another session's benchmark using three cores),
 at 1,351 ms on the rerun, whose load climbed to 29 when another
 session's tests started during it (its in-session Append took 864
 ms against 468), and passed at 936 ms on a third run at load 3.8 and
 2.8, every other row as before. The rest: history's 200 OCR
 versions of 1,000 pages at most 236 MB (≤ 256 MB, asserted; slice
 19: 236); AddMark 0.020 ms and a full reorder 0.962 ms at 20,000
 marks (< 1 and < 2 ms); 7,000 `SetOcrText` in one transaction
 lowered in 48.6 ms (the first 66.8 ms, blocks read cold), committed
 in 13.8 ms, an OCR run of 700 flushes 109 ms; 1,000 mark adds on the
 heavy 0.034 ms (blank 0.022 ms; the last 100 within 2× and 1.5×);
 ready on 20,000 sheaf marks 508 ms (< 1 s); their Rewrite 1,541 ms;
 Trim White on the synthetic 40.9 s (no gate); the clean copy 319 ms
 and 1,060 ms (no gate). The Rewrite against slice 8's writer
 (`rewrites_on_the_synthetic_and_heavy_7k`, each fixture in a
 process of its own, one run): synthetic 146 ms, 1.09×; heavy 792
 ms, 1.12×, under the 1.2× flag (slice 19: 1.09× and 1.09× over
 five); the gates' own Rewrite parts 147 ms (1.10×) and 779 ms
 (1.10×); RSS added 50.6 and 253 MB (≤ 1.5× slice 8's).
   - **The corpus** (`../pdf-corpus`, `SHEAF_CORPUS` unset; no "skipped:
 no corpus" line; debug builds, optimized), every count slice 19's:
 materialize_corpus's identity 4,389 of 4,392 written, 2,269,688
 primary objects surviving, 919,465 copied as they were, 432 changed
 by `keep_only`, 0 failed; 2,901 forms without their first page, 0
 failed; every page duplicated, 4,389, 0 failed; inserted twice into
 the 1040, 1,098, 0 failed; four times, 0 differ; the logo, 0 of
 2,037,600 bytes differ. append_corpus: 4,389 marked, 3,770
 appended, 259 rewritten (211 RemovalCheck, 34 BaseObjectsMissing,
 14 BaseUnsound), 360 asked, 0 failed, 0 derived vetoes and 211 on the
 files' own objects; the oracle finds text gone from 2,899 of 2,901
 (none from 0016064 and 0112654). encrypted_corpus: the Rewrite keeps
 573 of 573 and the Append 572 (0630899 declined). source_corpus
 passes. The export tier (`SHEAF_EXPORT_ONLY`) on slice 19's seven
 files (the 1040, 0025448, 0070616, 0187215, 0442193, 0630899,
 0861328): 7 of 7, the one failure its allowed one (0070616's extract,
 `NoAppearance(["Gruppieren4"])`). The full ignored tiers are the
 pre-merge run's.
   - **Suites:** `cargo test --workspace` 1,703 passed, 0 failed, 57
 ignored, and 1,707 after the review's four new tests (slice 19:
 1,654 and 56; ignored +1, a doc example the SDK
 marks `ignore`, now one each in ops.rs and view.rs where batch.rs had
 one). The release properties pass: Append ≡ Rewrite at 1,024 cases
 and the replay at 4,096 (89 s together), no seed added. The interop
 tier (qpdf; pdf.js 6.4.299 on node 24.22): 19 of 19 with
 `--include-ignored`. `qpdf --check` clean on a 1040 marked by `sheaf
 edit` (a note, its reply and a tickmark), its comment summary in
 both layouts, and the 1040 with those comments imported from the
 FDF, the XFDF and the marked PDF (each import "3 added", each
 listing the thread and the tickmark); the FDF itself has no page
 tree for qpdf. fmt and clippy (`-D warnings`, all targets) clean,
 and with sheaf-app's Linux-only gates switched off
 (`nonlinux-clippy.sh`, every file restored and checked); clippy
 (`-D warnings`) for `x86_64-pc-windows-msvc` on sheaf-edit,
 sheaf-plugin-api, sheaf-doc, sheaf-ops, sheaf-render and sheaf-print
 with all targets and sheaf-cli without its OCR engines;
 `cargo build -p sheaf-edit --target wasm32-wasip2` and `cargo check
 -p sheaf-app --no-default-features` pass. CI's plugin loop (each
 plugin's and the fixture's fmt and wasm32-wasip2 clippy, each
 plugin's `cargo test`, `scripts/build-plugins.sh`) passes,
 `--locked` and offline.
   - **Plugin wasm** (release, wasm32-wasip2, bytes, 0.3 → 0.4): amount-
 crossref 123,272 → 293,794; bookmark-from-pattern 617,027 →
 716,730; currency-convert 274,413 → 379,636; duplicate-pages
 169,135 → 306,630; subtotal 86,967 → 90,576; tax-form-bookmarks
 552,471 → 709,429; the test fixture 342,318 → 405,622 (the 0.2
 fixture's 164,420 gone). They pull in sheaf-edit, serde_json and
 schemars now.
   - **Bytes before and after step 5,** on a deterministic dump (seeded
 ids, imported names and dates, every mark kind holding a library
 stamp page and an attachment; the step-4 library rebuilt from the
 step's own copies, restored and checked with `cmp`):
 byte-identical, the Rewrite and the copy-Append of that document and
 both XFDF exports (the emitter and `gather` unchanged); different, as
 expected, the FDF exports (renumbered, holding only what the
 comments reach; read back, the same XFDF and marks, 4 and 30
 comments), the four summaries (builder order and the page-form
 copy), the 25 pinned pages (numbering) and the two tape pictures
 (`write_built` for `save_to`). Every changed PDF passes `qpdf
 --check`, renders pixel-identically with pdftoppm at 72 dpi (88
 pages) and reads the same in pdf.js (page counts and text).

   Checked by tests:
   - **Groundwork:** sheaf-edit's reply round trips and unknown-field
 refusals; `stale_tests`' `a_handed_out_relowering_that_makes_more_
 conflicts` and `a_handed_out_mark_lands_beside_an_unrelated_edit_
 with_its_id`; `change_tests`' `a_change_is_as_deep_as_its_commit`;
 tx tests' `only_a_plugin_granted_identity_learns_who_the_user_is`,
 `a_published_plugin_tx_is_on_the_current_version`,
 `a_batch_preview_leaves_the_transaction_as_it_was`; inputs'
 `a_plugin_render_without_markup_is_print_preview_s`; manifest's
 `a_plugin_built_for_newer_edits_is_refused` and
 `identity_is_a_permission_asked_for`; change.rs'
 `a_change_is_told_by_its_scopes` and
 `coalesced_changes_carry_the_union_of_their_pages`,
 `changes_merge_only_with_those_alike_to_answer` (review).
   - **The world:** tests/one_world.rs; host.rs's load test of
 `edit-schema = "1.1"`; the frame tests in PageSpace on `turned_doc`
 (after a sheaf turn too).
   - **Submits, rollback, staleness, caps, privacy, assets, reads**
 (host.rs, 51 tests): `a_submit_s_ids_are_what_its_edits_keep`,
 `an_envelope_with_a_label_of_its_own_is_invalid`,
 `a_crash_after_two_submits_leaves_no_version`,
 `a_crash_after_a_commit_takes_its_version_away` (truncate),
 `a_crash_after_the_user_edits_takes_back_only_the_plugin_s_edits`
 (revert), `a_total_of_tapes_changed_meanwhile_doesnt_land` (a
 Conflict, "“Recheck tape” changed the same data", and an
 `EditRefused` follow-up), `a_total_lands_beside_an_edit_to_a_page_
 it_didn_t_read`, `what_a_plugin_may_not_do_is_refused` (InsertPages
 User, a comment's delete and flatten Approval, a raw blob User),
 `a_plugin_learns_the_user_s_name_only_when_granted`,
 `an_uploaded_image_becomes_a_page_stamp` (after a `commit()` too),
 `a_call_reads_its_own_edits_and_doesn_t_decide_by_them`,
 `amount_crossref_refuses_cached_ids_after_a_reopen`, and the bundled
 plugins' tests, turned pages among them.
   - **Events and the app:** host.rs' `answers_to_answers_stop` (echo-a
 and echo-b, mark and commit modes), `an_answer_to_an_undo_is_
 refused`, `waiting_on_a_commit_is_free`; the app's
 `changes_made_while_a_plugin_is_busy_wait_merged`,
 `a_users_edit_waits_apart_from_changes_it_may_not_answer` (review),
 `a_plugin_isn_t_sent_its_own_changes`,
 `a_refused_call_is_told_why_only_when_it_asks_to_be`,
 `a_call_s_commit_waits_for_the_ui_thread_or_is_told_its_document_
 closed`, `a_plugin_knows_the_user_s_name_only_once_allowed`, and on
 Linux `a_call_s_commit_lands_through_the_view_and_its_next_edits_
 follow_it`, `a_failed_call_takes_back_only_its_own_commits_through_
 the_view` and the rewritten `a_call_lands_only_on_the_document_it_
 read`.
   - **PDFs made in memory:** `built_pages_share_their_fonts_and_read_
 back`, `another_pdf_s_comments_are_read_from_it`,
 `a_page_form_copies_its_resources_alone`; in review
 `a_stamp_is_sized_as_its_form_draws_it` and
 `an_image_stamp_is_made_within_its_budget`.
   - **The model's ids:** tests/object_ids.rs' four tests.
   - **Docs:** tests/docs_examples.rs.
   - **Fault checks** (each restored from the step's own copy and
 checked with `cmp`; no proptest seed added by any): step 1's
 equality check removed (the handed-out test fails); step 3's uploads
 not granted again after a commit, the `Key::Annot` read in
 `annotations()` removed, the raw-blob refusal removed, the identity
 branch of `Roles::of` swapped (each caught by its test); step 4's
 no depth refusal, no undo refusal, a mid-call commit at depth 0, no
 `edit-refused` refusal, that refusal still followed up, no deadline
 credit, a merger without `merge`, recipients without the `from`
 filter, follow-ups without the subscription check, and
 `commit_call` without the cell guard (each caught); step 5's fresh
 `ResObjects` a page, comments without the subtype filter or the
 password, and a page form copying the page's other keys (each
 caught); step 6's probe naming `ObjectId` in tx/flatten.rs and an
 orphan file (both caught); step 7's example op swapped for
 `flatten-annotations` (caught: "example 3: refused: that needs your
 approval").

   For Greg:
   - **M10's rewrite (plan/m10-forms/README.md slices 6+), what changed in each**,
 for your review before M10 resumes (every slice keeps its number,
 checks and proposals; added checks are marked):
 - *Status and intro:* M10 resumes after all of M10.5; the slices'
   intro explains the rewrite and the "On the edit API" bullets.
 - *6:* a pure `appearance::text(field, widget, display, &FormDoc,
   font) -> FormRes` on `ContentWriter`/`Font`, for the emitter and
   the realizer; encoders as Fonts; `/DR` handles from `Source::form`;
   `WidgetProps.border`; the spike settled. Added: a realized page
   and the saved file draw a changed field alike.
 - *7:* `Res::Font(Fallback)`, a late-bound GID-preserving Noto
   subset, a new subset object each Append; NFC, MaxLen and the
   refusal in `SetFieldValues`' lowering; it ends the `'?'` interim.
   Added: Unicode in a note and a header; an Append after new glyphs.
 - *8:* pure generators; group `/AS` rules as `SetFieldValues`
   lowering rules, tested alone until slice 9 adds the op.
 - *9:* one commit; `SetFieldValues` lowers to `Put Field`/`Put
   Annot`, no dirty tracking or re-binding; the save rules
   (REPLACES_TEXT, UR3 within its TransformParams, the
   earlier-revisions fact, `rules::plan` transforms, §9.3's asks with
   "Append anyway", `EditError::Locked`); the encryption notice gone,
   its check replaced by "encryption kept both ways and through
   recovery". Added: "Append anyway" and Locked.
 - *10:* PageSpace hit tests in `page_annots` order; `KeyPatch`/
   `PatchValue`, `PageDef.keys` for `/Tabs` with its role row; `/S`
   from `materialize::kept`; pending commits flushed before plugin
   calls. Added: a widget on a page turned in sheaf; a plugin sees
   the focused field's value.
 - *11:* `GroupKind::Typing` (`SetFieldValues` amends); `ViewInputs`
   hides the focused widget. Added: one focus session is one undo
   step.
 - *12:* renamed "marks drawn by PDFium": `MarkDraw::ByEngine`;
   deletes `annot_elements`, `stamp_preview.rs` and the tape pictures
   (D53); FFLDraw decided; carries slice 19's and this slice's Found
   on Add from PDF Page… Added: a mark on screen matches its saved
   appearance. Its no-disk check (in review) is no new file under the
   temp directory or sheaf's state and cache folders, the formjs
   compiled guest aside, where it named `SHEAF_CACHE_DIR`.
 - *13:* `ResetFields` as one Tx; Hide is `PatchWidgets { flags }`.
 - *14:* model reads; `TextOpts` gains `fields: true`. Added: plugin
   text still leaves values out.
 - *15:* `FlattenFields` on `FlattenAnnotations`' machinery (a
   `Placements` layer of `FormRef::Pinned` with `PinnedAp`'s widget
   inputs, removal through `widget.field`, the FormDoc emptied,
   `removed_annots` to `keep_only`); `FormBring::Flatten`;
   `SignatureNotValidHere` fields may flatten; print adds
   `FlattenFields`.
 - *16–17:* importers make `SetFieldValues` batches committed
   `Validate`; exporters are queries.
 - *18:* runs in the host's resolve stage, with decision reads.
 - *19–22:* `Actor::FormScript`, `FormsHost::resolve` committing
   `Validate` into the Typing group, `sync` on `ChangeSet.fields`, a
   save waiting for a running cascade; 20 added: undo, plugin fills
   and imports reach the sandbox through `sync`; 21 and 22 commit
   into the Typing group; 22's `PageSource::Template` comes with
   `spawn` (Deviations).
 - *23–26:* the field ops, `FieldNode.patch`, `PatchValue` and the
   role rows; 24 `PatchFields` and `PatchWidgets` beyond slice 13's
   flags, rename and move; 25 `/AA` and `/A` patches (added: removing
   a script forces a Rewrite); 26 `SetTabOrder`/
   `SetCalculationOrder`, copies by locals, untagged until M14.
 - *27–28:* queries over the text service and the engine pool;
   accepting is one Tx.
 - *29:* renamed "Plugin API 0.5": field ops join the JSON `Op` as
   `Cap::Plugin` (a flatten needs approval); field reads behind
   `fields`; no 0.3 or 0.4 served; docs/forms.md drops "encryption
   until M11"; m11 already 0.6. Added: a Checks bullet.
 - *Proposals:* 7's "encrypted: only Rewrite" gone, "every way out"
   at `forms::rules::plan` and the export preflight; 8's re-binding
   and dirty tracking gone, incremental.rs now the emitter; 10's slot
   hiding replaced by the realizer and `ViewInputs`, its no-disk test
   restated as slice 12's (review); 18 as
   transforms; 22's encryption warning gone; 23 now 0.5. Decision 2
   notes M10 takes 0.5 and M11 0.6; the encryption lines of Settled
   at kickoff, Asks and Not in M10 marked moot.
   - **The `ObjectId` allowlist** (Deviations): allowlist the open's fact
 readers and the other file-level readers by file, as now, or move
 them under source/ (a large churn of public paths). generate/saved.rs
 is allowlisted rather than put behind `Source` methods, since the
 cached facts can't hold `ObjRef`s, and the page key is `PageObj`.
   - **No plugin exports in 0.4:** `actor_exports`' plugin arm has no
 WIT producer, and a plugin's flatten is refused as Approval (D46).
 Say if 0.5 should offer an export call under its own consent.
   - **`document.path` stays in 0.4,** though §14.3's sketch leaves it
 out: a path can hold the user's login name (`/home/greg/…`), which
 roles otherwise keep from a plugin. Recommended: tell it only under
 `identity`, in 0.5. Not changed here; README and docs/plugins.md
 say a plugin is told the path (review).
   - **The SDK isn't publishable yet:** sheaf-plugin-api reads its WIT
 from `../../wit`, outside its crate, and depends on sheaf-edit,
 unpublished (its workspace dependency now carries a version, so
 that part only needs sheaf-edit published first). docs/plugins.md
 shows a path dependency until then. Say when the SDK should go to
 crates.io.
   - **The `Objects` eager arm stays** until a Rewrite needn't load the
 file whole (F15) and the form check reads lazily.
   - **Behavior to confirm:** answers to `edit-refused` are refused and
 not followed up; only changes alike to answer merge while a plugin
 is busy, so it may get two `changed` for one wait (review); the depth refusal's wording; subtotal invalidating
 by the text's scopes; `PENDING` keyed by path and instance; roles
 under `identity` naming everyone but installed plugins; `pages()`
 reading `Labels`; a newer `edit-schema` refused at the manifest's
 parse; bundled wasm 1.0–2.4× larger.
   - **Wording, yours:** "the user undid this"; "it answers a plugin's
 answer to a change, and those stop there"; "it answers a refusal of
 its edits"; "commit: its document was closed"; "… was stopped, so
 it changed nothing" and "… was stopped; its partial edits were
 kept"; "the document was opened again; find again"; "a plugin's
 edits are called as its submit says: the envelope has no label";
 "these ops are written in version …, and the manifest says …";
 "the plugin's manifest doesn't say which edits it writes
 (`edit-schema`)"; "built for edits 1.1; this sheaf reads up to
 1.0"; "this call's edits land when it returns, not before"; "a
 call's uploads hold at most 64 MiB"; "Know your name".
   - **The other plans:** m10.md slice 22's `PageSource::Template` with
 `spawn`; slice 29's field ops for plugins; agent.md's "per agent
 session"; m11.md slice 1 renamed, keeping bug 12.
   - **As before:** option A (a lead window) for proposal 8's accepted
 misses, and the flattened export's RSS gate (unattainable by
 construction, F15).

   Left for later:
   - **M10 slice 12:** a stamp made from a page (`stamp_lib::one_page`,
 Add from PDF Page…) keeps the page's `/Annots` and reads the
 unsaved state, not the file loaded whole (in m10.md's slice 12).
   - **M10 slice 29:** Plugin API 0.5, field ops and field reads.
   - **M11:** Plugin API 0.6 (its proposal 15), per-export security and
 the redaction resolve stage, as m11.md now says.
   - **M16:** the agent's edits through `Document::plan`/`PlanHandle`,
 on the same envelope and decision reads (agent.md).
   - **Greg:** the allowlist, `document.path`, plugin exports, option A
 and the flatten gate (For Greg).
   - **Follow-ups:** the `Closure::stale` probe (slice 11's Unprobed),
 untouched: no step changed that walk. The Found items above.

   Earlier slices' "Slice 20" bullets, as met:
   - **Slice 2's** (m11.md waits for slice 20; the AES-256 `/Length 256`
 bullet): applied, in m11.md's proposal 10 and slice 2.
   - **Slice 4's** (WIT 0.4's `opened (when ready)`): kept as it was
 (`plugins::Waiting`, `release_plugin_calls`).
   - **Slice 5's** (to-view and from-view; bug 20's page-info half):
 `page-info` carries both matrices, its size and coordinates one
 frame.
   - **Slice 11's** (Op JSON across WIT 0.4, sheaf-plugin-api on
 sheaf-edit, `mark_handed_out`, the adapter deleted): done.
   - **Slice 12's** (items done early verified; `mark_handed_out` and
 `created` equality; `ChangeSet.depth`; 0.4's reads and events;
 `commit()` mid-call with the next Tx on its version): done; the
 early items (`edit_source`, decision reads, the lazy base,
 `DocContext`'s snapshot) carried into the transaction.
   - **Slice 13's** (`without-markup`; wit/0.2's render doc): done, and
 wit/0.2 deleted.
   - **Slice 14's** (to-view and from-view for text; narrowing a text
 read's generators to those that can reach the page): text is in
 PageSpace; the generators aren't narrowed (all of them, as slice
 14 recorded), which stays correct.
   - **Slice 18's** (0.2's `page-text-layer`; OCR in 0.4 with its
 permissions): deleted with 0.2; 0.4 keeps `ocr.recognize` alone,
 since §14.3 defers OCR ops (`SetOcrText` is `Cap::User`).
   - **Slice 19's** (clipboard and assets; plugins' flatten and exports;
 the to_lopdf.rs and Objects-seam rows; the final docs pass):
 `assets.upload` and no clipboard or export WIT (Deviations);
 to_lopdf.rs deleted into `serialize::built`; the seam's eager arm
 kept for whole-file loads (ledger); the docs pass done.
