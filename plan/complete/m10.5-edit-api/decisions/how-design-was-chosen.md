# M10.5: How the design was chosen

Part of [the M10.5 decisions](README.md).


Sources for this section, all from the 2026-10-15 planning session: the
ten survey reports (`survey/*.md`), the five designs (`design-*.md`),
the three judgments (`judgments.md`) and the decision log at the head of
`synthesis-meta.md`. The plan says what was decided. This section
records how the candidates compared and what each one contributed. Where
the plan's final text differs from these earlier artifacts, the plan
wins, and the difference is noted ("Plan:").

### How the survey was done

Ten readers surveyed the code read-only at HEAD `32723da` (branch
`m10`). M10 slice 3 was committed and slice 4 was uncommitted in the
tree, so line references are at that HEAD, and readers who covered the
slice 4 work described it separately (the brief marks it WIP). Each
reader wrote a markdown report
and a JSON companion, and marked each claim as verified (read in the
code or upstream source) or inferred. The ten scopes:
- **model-history:** the sheaf-doc model, edits and history (model.rs,
  history.rs, describe.rs, lib.rs, docs/document-model.md).
- **save-materialize:** save.rs, incremental.rs, kept.rs, recovery.rs,
  file.rs and generated.rs; the content writers they call (text_layer,
  stamps, watermark, `annot::to_pdf_in`); and the producers of derived
  sources (derive.rs, crop.rs, page_insert.rs).
- **content-features:** everything that changes what a page shows or
  adds objects to the file:
  - crop, watermark and background, header/footer and Bates, and strip;
  - OCR layers, tapes, stamps, connectors, measurements, labels and
    attachments;
  - blank pages, images-to-PDF and scan; insert, replace, combine and
    split;
  - layers, flatten, clean copy, and print and export;
  - the appearance generators and the shared content, font and encoding
    helpers.
- **forms:** the M10 field model, forms carried along by page tools, and
  what M10 slices 6–29 need from the API.
- **app-consumers:** how the app and the CLI build, preview, apply, undo
  and save edits. That covers gestures, ids, threads, selection, jobs,
  refresh, and the headless path from arguments to save.
- **plugins-agent:** WIT 0.2 and 0.3, the host runtime and batch
  application, the SDK's `Batch`, the bundled plugins, document events,
  the agent-panel plan and the merge-later plan.
- **render:** rendering and text extraction, meaning what it takes to
  show edited content and read text from it.
- **future-milestones:** M11–M15 and Pending, read from ROADMAP.md,
  plan/m11-security/README.md, the README tables and docs/prior-art, and checked
  against the code.
- **prior-art-pdf:** MuPDF, PDFium, pdf.js, iText 7 with pdfSweep,
  PDFBox, qpdf/pikepdf, lopdf, pdf-writer/krilla, hayro, pdf_oxide and
  pdfium-render, plus the redaction-leak literature and the signature
  rules. Upstream sources were fetched that day. AGPL code is a source
  of behavior only.
- **prior-art-editors:** ProseMirror, Slate, Lexical, tldraw, Figma, Xi,
  Yjs/Automerge, Excalidraw, Photoshop/Affinity, Git/LFS,
  OCAF/FreeCAD/Onshape, CodeMirror 6, VS Code/LSP, MuPDF's journal and
  pdf.js. Each pattern was mapped onto sheaf's snapshot-and-edit-tree
  history.

The ten reports were merged into one brief (`brief.md`), and all five
designers worked from it. The plan's research list also includes PDFium
measurements on a synthetic 7,000-page file and on 402 real files of 4
MB or more.

### The five designs

Each design took one angle on the same brief, covered every area of it,
and proposed its own slices. Scores are given in lens order: correctness
and fit / cleanliness and deletion / feasibility and delivery.

**model-definition (14 slices).**
- **Thesis.** `DocState` becomes the complete definition of the
  document, and one emitter derives every PDF object from it, on screen
  and on disk.
  - A page is a definition over an immutable, lazily read source page:
    its geometry, an ordered content stack of filters and static layers,
    and its annotation list.
  - Annotations, field nodes, bookmarks and the header/footer, Bates and
    watermark generators are keyed records.
  - Intents lower once into two primitives, `Order` splices and
    `Put { key, before, after }` over 9 key kinds, and history replays
    only those.
  - A save never touches the model. It attaches a `SaveRecord` that maps
    model objects to the file's object numbers. Surviving primary
    objects keep their numbers, and untouched objects are copied as raw
    bytes.
  - The realizer serializes the same emission as an in-memory append
    section, one per 32-page group.
- **Other choices:**
  - live page tokens with frozen Bates, where a change to a live block
    already in the saved file forced a Rewrite;
  - stale bases re-lowered against the cursor, with `(Key, Part)` read
    sets;
  - a hand-written WIT mirror checked by a proptest;
  - a plugin `submit` that blocks while the UI thread commits;
  - v1 `/SheafAnnot` marks opened as foreign;
  - unencodable text refused at commit;
  - encryption added to the old writers in slice 2.
- **Strongest ideas, as the judges saw them:**
  - a clean, closed primitive layer ("the writer needs about two match
    arms");
  - saves as records with stable numbers and raw passthrough, and the
    only design that explicitly kept object-stream members out of the
    raw copy;
  - generators as one document-level rule for the binder, extracts and
    print;
  - a fixed `PageDef.copy` index, the only design that stopped
    page-object ownership from flipping when a duplicate is moved before
    its original.
- **Weaknesses raised:**
  - Refusing non-cp1252 FreeText from slice 6 is a visible regression.
  - The M12 hooks were asserted but not designed.
  - Dropping a `/SheafBlock` that no generator backs could silently
    remove visible content.
  - Live tokens pull in a lot of machinery: position-dependent flags,
    drawn digests, layout that must stay identical across builds, and a
    7,000-page Rewrite on every reorder after a save.
  - Slice 2's encryption work on the old writers is thrown away when
    slice 4 deletes them.
  - Slice 3 is enormous and needs a risky legacy projection.
- **Fatal flaw.** None under correctness or cleanliness. Under
  feasibility, `SaveRecord.map` treated base objects as identity. In the
  sequence delete pages, Rewrite (where `keep_only` prunes them), undo,
  save, the final Append references freed objects.
- **Scores:** 7.5 / 8 / 7.5.

**risk-first (18 slices plus a cut line).**
- **Thesis.** Make a wrong undo and a leaky save impossible, or at least
  loud.
  - Intents lower to a closed set of typed Prims: page-list splices plus
    `Put<K,V>` per collection, 21 variants, each carrying before- and
    after-values. Replay asserts every before-value and never re-lowers.
  - Each Prim declares its effects before it is applied, including a
    `SigClass` mapped to DocMDP. Consequences fold along the path from
    the saved version to the target.
  - An object-level removal check on every Append backs up the
    content-based classification, "so a misclassified edit can only make
    a save slower, never leakier".
  - Page content is a layered `ContentDef` (a base that is the source,
    filtered, pinned or empty) plus document-level generators.
  - One emitter feeds Rewrite, Append and overlays in 64-page groups.
    Encryption lives in that one serializer.
  - Saves are outputs, recorded as a `SavedFile` with an object map.
  - Exports use a separate `Transform` type that the binder can't
    commit, through a single `export::run`.
- **Also proposed:**
  - the registry rule: cross-cutting functions destructure `DocState`
    exhaustively, with no `..`;
  - clippy bans on lossy lopdf calls and temp files;
  - one `Limits` value and fuzz targets;
  - vendored lopdf `Real` as `f64`;
  - `BlobCollision` on xxh3 hits;
  - inserted pages copied into an immutable `Slice`;
  - M12 hooks (`Profile.reserve`, byte offsets, `SigImpact`) built and
    tested;
  - PDFium-drawn marks with a Noto fallback, behind a cut line (slice
    17).
- **Strongest ideas.** The correctness judge called it "the strongest
  correctness story": checked replay, the fail-closed removal backstop,
  the registry rule, the most complete prim set for later milestones,
  and the only design that specified and tested M12 hooks inside M10.5.
- **Weaknesses raised:**
  - It had the largest scope.
  - The removal check took its old side from `emit(saved_state)`, not
    from the file's actual objects.
  - A generator recognized after a reopen is frozen ("Baked"), while the
    same generator made in-session is live.
  - Any header text change forces a rewrite, and so a signed-file ask.
  - Slice 7's removal check needs the lexer, which arrives only in slice
    9.
  - Forms have no defined home in slices 6–13.
  - The `Slice` copy brings back an eager parse.
  - The cleanliness judge counted two removal mechanisms, a second
    public enum (`Transform`), speculative catalog, security and file
    tables, and 21 prim variants "where a single keyed Set would do".
- **Fatal flaw.** None under correctness or cleanliness. Under
  feasibility, the same no-rebase gap: Append writes the closure of new
  keys, not of the base objects a pruning Rewrite freed.
- **Scores:** 8.5 / 6.5 / 6.5.

**consumer-first (16 slices).**
- **Thesis.** Design the API from its callers.
  - Every surface builds a serde `Tx` of semantic ops.
  - `plan(&Tx)` returns a `Plan` holding the after-state, describe
    lines, consequences, capability and created ids. One object serves
    as dry run, live preview, approval card and menu check.
  - `commit(plan)` reuses the computed state when the base is still
    current.
  - Commit, amend, undo, redo, jump and save all return the same
    `Transition { change: ChangeSet }`, so the app has one refresh
    routine.
- **Underneath:**
  - about 17 model-level prims, storing after-values only;
  - read sets recorded by the lowering context;
  - `Decorations` in `DocState`, with `/SheafRev`;
  - overlays in 256-page groups;
  - PDFium drawing committed marks inside M10.5, which deletes the GPUI
    mark renderer;
  - a WIT `tx` resource with one method per op;
  - a `mark_saved` that rebases exactly the saved version, while later
    versions stay on the old base.
- **Strongest ideas.** "The best consumer API": one Plan object and one
  Transition. Also an Append that rewrites `/Pages` nodes in place (a
  flat 7,000-page reorder writes one object), and the most app-side
  deletion.
- **Weaknesses raised:**
  - Steps store no before-values.
  - It has no catalog, metadata, security or embedded-file prims.
  - The rotation delta is kept.
  - Non-cp1252 drawn text is refused at commit.
  - 256-page groups throw away warm caches widely.
  - Slice 8 is an XL grab-bag.
  - Rebinding every origin on every save costs O(model).
  - The `tx` resource re-creates the hand mirror one method at a time.
- **Fatal flaws:**
  - Correctness: `mark_saved` rewrites the saved version V in place,
    while versions made during the save stay on the old base. If one of
    those versions has its snapshot trimmed, its prims replay onto V′,
    and undo can be wrong.
  - Cleanliness: the save keeps a rebase, the same shape as
    `rebase_to_saved`.
  - None under feasibility.
- **Scores:** 6.5 / 6 / 6.8.

**object-delta (15 slices).**
- **Thesis.** Put the primitive layer at the PDF object level: a
  persistent copy-on-write overlay of objects (`PdfDelta`) over
  immutable, lazily parsed sources.
  - It has nine ops: `Put` and `Drop` of objects keyed by `ObjRef` (Own,
    Src or New), plus typed spines for the page tree, outline, field
    tree, `/Annots`, `/Contents` and number trees.
  - Every write carries an `Owner` entity and a `Role`.
  - Its argument: with model-level prims, the save is "a second
    translation from model to objects, and that translation is exactly
    what drifts today".
- **How it works:**
  - Incremental saves append exactly the changed addresses.
  - A rewrite keeps object numbers and returns a `SaveMap`.
  - The save rebases the version it wrote, and later versions are
    re-derived by substituting the map into their prims.
  - The realizer keeps one overlay per (source, generation).
  - The base stays a lazy `SourceModel`, with no gated open.
  - The core is built headless (slices 4–11), and the app moves onto it
    in one cutover (slice 12).
- **Strongest ideas.** "The best structural fit for M11, M12 and M14
  auditing": Owner- and Role-tagged writes make "changes since signing",
  object-level audits and "all uses" edits native. Also:
  - a fail-closed removal allowlist by role and key;
  - structural copy-on-write with a reverse-users index;
  - the only explicit "Append anyway" on signed files;
  - the earliest encrypted-section spike (slice 2).
- **Weaknesses raised:**
  - Every reader merges two layers, which is the Base+Overrides pattern
    the design says it avoids.
  - A rewrite builds a full lopdf Document, so dict reals go through f32
    and everything is re-encrypted.
  - Marks become Synth inputs that every consumer has to decode.
  - It has the highest concept count.
- **Fatal flaws:**
  - Correctness: `EntityId`s are derived from objects' birth addresses.
    R28 forbids ids based on ObjectId, and using the same source object
    twice collides. Per-generation overlays re-serialize every edited
    page of a source on each commit after a document-wide edit, which
    fails M13 latency and R72.
  - Cleanliness: "the primitive layer is the PDF object graph".
  - Feasibility: the big-bang cutover, and the same per-generation
    overlays.
- **Scores:** 6 / 4 / 4.5.

**minimal-delivery (17 slices, sized S/M/L/XL, about 46 points).**
- **Thesis.** A small core built to delete code.
  - The document is a few id-keyed tables of immutable records. The page
    record holds its content definition: base, boxes, absolute rotation,
    `BaseContent` (Source, Filtered or Empty) and under/over layers.
  - Every edit lowers to one primitive, `Set { key, Option<record> }`,
    over 8 tables. Ordered collections are record values.
  - Steps store after-only deltas, so replay is `apply(delta)` and can't
    fail.
  - Effects are a pure function `effects(key, old, new)`, so no op can
    forget to declare one.
  - One writer has three targets (Full, Append, Overlay), and overlays
    are per page.
  - Saves never rebase.
  - Header/footer and Bates tokens are all frozen at apply, with an
    Update action and an out-of-date indicator.
  - Ops cross WIT as JSON of the exact Rust `Op`. A plugin call is one
    Tx, lowered on the plugin thread and committed when the call
    returns.
  - Sources and blobs share one content-addressed `Store`.
  - Encryption is deferred to M11 behind a hook, and encrypted sources
    render over an in-memory decrypted image.
  - Every slice must delete something. Its ledger came to about 7,000
    lines out and 4,500 in.
- **Strongest ideas.** "The smallest coherent core": one case in every
  consumer, no mirrors at all, and the most code deleted. Also the best
  delivery plan: sizes, a deletion in every slice, and every piece of
  transitional code listed with the slice that deletes it.
- **Weaknesses raised:**
  - Steps store no before-values.
  - Coarse keys (Outline, Labels, Form and Order as single records) make
    plugin and agent edits conflict with unrelated user edits.
  - It has no catalog, security or attachment keys.
  - Frozen page numbers print "Page 5 of 7000" in a 3-page extract, and
    scope resolved at apply leaves pages inserted later without the
    header, so T14 stays open.
  - Per-page overlays risk about 7,000 cold engine opens for a thumbnail
    strip after a document-wide header or watermark.
  - Ids returned before the end-of-call commit can vanish on a conflict
    (R103).
  - Blob ids are unchecked xxh3, and reals stay f32.
  - The plugin view-frame contract gives the shared `Op` JSON two
    coordinate meanings.
  - Page-object ownership depends on page order.
- **Fatal flaws:**
  - Correctness: R59 and R60 are unmet. Saves stay plaintext, and
    encrypted files get no recovery.
  - Feasibility: the no-rebase gap.
  - None under cleanliness.
- **Scores:** 5 / 8.5 / 7.

### Scores and ranks

The judges gave no combined score. The sum and the rank total below were
computed for this archive.

| Design | Correctness and fit | Cleanliness and deletion | Feasibility and delivery | Sum | Rank total |
|---|---|---|---|---|---|
| model-definition | 7.5 (2nd) | 8 (2nd) | 7.5 (1st) | 23.0 | 5 |
| risk-first | 8.5 (1st) | 6.5 (3rd) | 6.5 (4th) | 21.5 | 8 |
| minimal-delivery | 5 (5th) | 8.5 (1st) | 7 (2nd) | 20.5 | 8 |
| consumer-first | 6.5 (3rd) | 6 (4th) | 6.8 (3rd) | 19.3 | 10 |
| object-delta | 6 (4th) | 4 (5th) | 4.5 (5th) | 14.5 | 14 |

Fatal flaws by lens:

| Design | Correctness and fit | Cleanliness and deletion | Feasibility and delivery |
|---|---|---|---|
| model-definition | none | none | base objects mapped as identity, so an Append after an undo across a pruning Rewrite references freed objects |
| risk-first | none | none | the same no-rebase gap |
| minimal-delivery | R59/R60 unmet: no encryption kept, no encrypted recovery | none | the same no-rebase gap |
| consumer-first | in-place `mark_saved` breaks replay of versions made during a save | the save keeps a rebase | none |
| object-delta | birth-address ids (R28); per-generation overlays | the primitives are the PDF object graph | big-bang cutover; per-generation overlays |

### Where the judges agreed and split

**All three agreed on:**
- model-level primitives in history, with object-level emission only in
  the materializer (F1);
- saves without a rebase;
- one gated open (F10);
- keeping encryption in M10.5;
- GPUI drawing sheaf marks until the fallback font exists (F21).

**They split on:**

| Fork | Correctness and fit | Cleanliness and deletion | Feasibility and delivery | Where it ended |
|---|---|---|---|---|
| What a step stores (F2) | prims with before-values, asserted | after-only delta; before-values redundant | `Put` with before and after as Arc clones | before-values, asserted (proposal 12) |
| Primitive set | model level, plus a `SrcPatch` escape hatch for unmodeled objects | one `Set` over keyed tables | an `Order` splice plus one generic `Put` | one `Put` over 11 key kinds; no patch type until something produces one (§3.3) |
| Generated tokens (F6) | page tokens live; Bates frozen and persisted | all frozen, with Update | frozen with Update, "a product call for Greg" | merge: live, with `LIVE_REDRAW` appendable. **Plan:** open for Greg (proposal 8), behind slice 17's gate, falling back to frozen |
| Removal backstop (F13) | strict, plus an object-level check; Append anyway on signed files | one `effects` function; the check at most a test oracle | object-level check on every Append | the check on every Append and as an oracle on every Rewrite (§11.9) |
| Exports (F16) | a separate `Transform` enum | `Cap::Export`, no second enum | — | `Transform` (§6.1) |
| WIT shape (F7) | a Rust enum mirrored with total conversions; JSON acceptable | JSON of `Op` | JSON of `Op` | JSON (proposal 18) |
| Plugin delivery (F25) | a blocking submit committed on the UI thread | a per-call Tx on the plugin thread, committed at return | the same as cleanliness | a per-call Tx on the plugin thread with an optional blocking `commit()`; **Plan:** commits with `Relower` (§14.3) |
| Overlay granularity (F5) | (source, 32–64-page group) | per page first; groups only if a gate fails | about 32-page groups, tunable | GROUP set by slice 2's spike. **Plan's slice notes:** 128, scoped |
| Stale base (F24) | — | apply the unchanged delta if every read and written key is `ptr_eq` | — | merge: that rule. **Plan:** validate by value, with `Validate` or `Relower` and pinned ids (proposal 13) |
| Catalog-level keys | add now (Catalog, Security, Files, Resource) | speculative; no variant that nothing produces | — | none until produced; the registry rule makes adding them later compile-checked |
| Encryption placement | in the one serializer | in the one serializer | in the new serializer, not the old writers; spike early | serializer and saver slices 7, 9 and 10 (proposal 23) |
| Inserted-page sources | — | reference the lazy source; no `Slice` | reference the lazy source; no `Slice` | reference the source |

### Why model-definition was the base

The decision log's rationale: model-definition ranked 2nd, 2nd and 1st
across the three lenses. It was the only design near the top under all
three, and it had no fatal flaw the grafts couldn't fix.
- Its one fatal flaw, base objects mapped as identity, was shared by all
  three no-rebase designs. The feasibility judge's fix closes it "at the
  cost of a bitset per save", which became `SaveRecord.present`.
- The log names two other candidates for the base:
  - **risk-first:** "best on correctness, but 18 slices of speculative
    tables";
  - **minimal-delivery:** "cleanest, but defers encryption and freezes
    page tokens".
- consumer-first had fatal flaws under two lenses, both coming from its
  rebase. object-delta had fatal flaws under all three.

**Kept from model-definition:**
- `DocState` as the definition, with one emitter for screen and file;
- `PageDef` with its fixed copy index and content stack;
- document-level generators with `Scope::All`;
- `SaveRecord` with stable numbering and raw passthrough that excludes
  object-stream members;
- the lazy lopdf reader;
- group overlays loaded from two segments;
- PageSpace;
- ops that read immutable source facts;
- "no variant exists that nothing produces";
- GPUI drawing marks during M10.5.

**Overridden in model-definition:**
- `Order` plus `Put` became one `Put`.
- `(Key, Part)` reads became finer keys, with `Geom` and `PageAnnots`
  separate from `Page`.
- The hand-written WIT mirror became JSON.
- The blocking plugin submit became a plugin-thread Tx.
- `Cap::Export` became `Transform`.
- Encryption moved from the old writers into the new serializer.
- v1 marks are converted on read, not opened as foreign.
- Unencodable text gets a `'?'` in the drawing instead of a refusal.
- Unbacked sheaf blocks are kept, not dropped (§3.5).
- The single huge slice 3 was split.
- `StableKey` anchors are not recorded.

### What was grafted from the other four

The decision log's summary: "risk-first's checked replay, removal
backstop, registry rule, Transform type and M12 hooks;
minimal-delivery's single primitive, pure effects, JSON ops, Store and
delivery discipline; consumer-first's Tx-as-plan, single ChangeSet and
auto read sets; object-delta's early encrypted spike and Append-anyway."
In detail:

**From risk-first:**
- **Before-values asserted on replay.** A mismatch shows as "History is
  damaged at version N" (proposal 12, slice 1).
- **The object-level removal check on every Append,** vetoing to a
  Rewrite (§11.9). The merge took the check's old side from
  `emit(saved_state)` where the save map said sheaf wrote that key, and
  from the source object otherwise. That answered the correctness
  judge's objection to risk-first's version. **Plan:** the old side is
  the saved file itself, read through the lazy reader. Derived objects
  are judged on their record inputs and source objects on decoded text,
  and the check also runs as an oracle on every Rewrite.
- **The registry rule** (§2).
- **`Transform`, accepted only by the export transaction,** through one
  `export::run` (§6.1, §12). The merge sided with the correctness judge
  because "plugins never apply redactions" becomes a type-level fact
  "for about 30 lines".
- **Smaller pieces:** `f64` reals, `BlobCollision`, one `Limits` with
  fuzz targets, clippy `disallowed-methods`, Dense numbering for
  exports, foreign ids seeded by the source hash, and the pairing of
  PDFium-drawn marks with the fallback font (scheduled for M10 slice
  12).
- **M12 hooks** (`Profile.reserve`, byte offsets) were in the merge's
  serializer slice. **Plan:** dropped from M10.5 (proposal 38). Revision
  reading stays, and §20 records how M12 will sign.

**From minimal-delivery:**
- **The single primitive,** as `Put { key, before, after }` over 11 key
  kinds. This is minimal-delivery's one primitive with the before-values
  of risk-first and model-definition, and ordered collections are
  values.
- **Pure classification:** `classify(key, before, after)` (§7.1).
- **JSON of the Rust `Op` over WIT,** with `deny_unknown_fields` and a
  schemars schema (proposal 18).
- **The content-addressed Store** (§3.1).
- **The first slice records what changed,** on top of today's `compute`.
- **Delivery discipline:** sizes, a deletion headline per slice, and the
  transitional code ledger.
- **`SaveRecord.transforms`,** so an Append never repeats an XFA drop
  (§11.8).
- **The per-call plugin Tx lowered on the plugin thread,** with an
  optional blocking `commit()` added for R103's "final outcome" (§14.3).
- **The stale-base rule** with no re-lowering. **Plan:** replaced by
  value validation with `Validate` and `Relower` (proposal 13).
- **The interim `'?'` in mark appearances,** together with object-delta.

**From consumer-first:**
- **The Tx as the plan.** `Tx::preview()` returns the scratch state,
  lines, effects, consequences, created ids and caps. A commit at the
  base reuses `tx.state`, and preview ids come from the real `IdAlloc`
  (§8.5–8.6).
- **One `ChangeSet`** from commit, amend, undo, redo, jump and save.
- **Read sets recorded automatically** by the lowering accessors (§6.3).
- **`/Pages` nodes rewritten in place on Append** (§11.6).
- **Encryption built into the new serializer,** not the old writers
  (with risk-first; the feasibility verdict).
- **Groups for streaming producers,** previews for dialogs and drags.
- **Op JSON versioning,** a passing `"v": 1` that became the plan's
  `{"v":"1.N"}` envelope and `edit-schema` manifest key.
- **An explicit date for deleting the GPUI mark painter,** asked for by
  the cleanliness judge: M10 slice 12 (§10.4).

**From object-delta:**
- **The encrypted-section spike before anything depends on it:** slice
  2, spike A.
- **"Append anyway" on signed files with removals:**
  `AppendAnyway { stays_readable }` (§9.2).
- **Key-level patching** of a record that has a source object, keeping
  unknown keys (§3.6).
- **v1 `/SheafAnnot` conversion,** together with risk-first and
  minimal-delivery (proposal 21).
- **The reverse-users index** was in the merge. **Plan:** deferred to
  M13, with the first "all uses" op (§11.5, §20).
- **Owner/Role tags with a removal allowlist by role and key** were
  offered as a graft by the correctness judge. The merge let the
  `ObjKey` kind serve as an object's role for M12. **Plan:**
  - classification stays per model key (§7.1);
  - M12's "changes since signing" uses a content-based role classifier
    over later revisions, with the `ObjKey` mapping only as a test
    oracle (§20);
  - each future `KeyPatch` key brings its own classify row (§3.3).

### Missing from all five

These are gaps the judges found in every design.

| Gap (lens that raised it) | Where the plan answers it |
|---|---|
| Another tool edits a sheaf mark, and sheaf reopens it from stale `/SheafAnnot` JSON and reverts the edit (correctness) | §3.6: v2 carries a canonical staleness digest `"std"` of the standard keys, and a mismatch opens the mark as Foreign. For v1, `modified` is compared with `/M`. Tested in slice 5. |
| Frozen Bates numbers don't survive a save and reopen (correctness) | §3.5: each drawn block's `/SheafBlock` carries `/N`, and the frozen map is rebuilt from it. Checked in slice 17. |
| Host-resolved form payloads (AF, scripts) report no read set (correctness) | §6.3: `FormsHost::resolve` returns `(payload, reads)`. §8.5: form calculations commit with `Validate` on the calculation's inputs. |
| No accessibility policy for new marks and copied widgets on tagged files, R44 (correctness) | Deferred, and stated. Proposal 24: copies are untagged, `/StructParents` stays only on copy 0, and a Note appears when the source was tagged. §20: M14 adds `Key::StructElem` and a `/ParentTree` emitted from the model. |
| M15's Bates continuation across files (correctness) | §20 M15: `BatesStart::Continue`, resolved by the batch runner. Listed under Not in M10.5. |
| What signing does to history (correctness) | §20 M12: signing is two ordinary commits around one Append (`SignField`, a save with `Profile.reserve`, then `SetSignatureValue`), and it requires the plan to be an Append. The hooks themselves aren't built in M10.5 (proposal 38). |
| No-rebase saves don't record which primary objects survive a pruning Rewrite (cleanliness; the feasibility judge's fatal flaw for all three no-rebase designs) | §9.1: `SaveRecord.present`; `BaseObjectsMissing` forces a Rewrite; `SaveRecord.roots` supplies the unowned root keys. Slice 10 checks delete, save, undo, save, reopen under `qpdf --check`, and the Append ≡ Rewrite property test runs by default (§7.2). |
| Page-object ownership flips when a duplicate is moved before its original (cleanliness) | §3.3: a copy index fixed at creation. Found while planning, bug 3. |
| The history policy could shrink to one byte budget (cleanliness) | Partly. §8.7 prunes versions when `step_bytes + blob_bytes` exceed a byte budget, with a detached saved anchor. `MAX_VERSIONS` and `HistoryPolicy` (200 snapshots, 256 MB) stay (§8.1). |
| No single retired-symbols ledger test (cleanliness) | `tests/retired.rs` from slice 1, matched with a `syn` walk. Every slice adds the names it retires. |
| Persisted Op JSON has no version of its own (cleanliness) | §14.3: a `{"v":"1.N","ops":[…]}` envelope and an `edit-schema` manifest key. Only new read functions bump the WIT minor. |
| Slice 5 is committed, and `forms/rules.rs` mutates the output graph after the fact (cleanliness, feasibility) | §11.8: rewritten as the pure, plan-time `rules::plan` in slice 9. §1 starts from `a0955fc`. |
| Object streams: whether a Rewrite writes them, and the stale members left by raw-copying an object-stream container (feasibility) | §11.6: containers are never raw-copied, and their members are re-serialized as plain objects, as today. M10.5 writes no object streams; packing moves to M14 (proposal 37). The merge had packed them on xref-stream bases. **Plan wins.** |
| No spike of the overlay realizer on real heavy files (feasibility) | Slice 2, spike B: the 402-file pool sample, forms after an in-place page replacement, and xref-stream, hybrid, linearized, damaged and leading-junk bases, with written pass criteria. Also the heavy and scanned 7k fixtures. |
| Realization over damaged bases (feasibility) | §10.1: damaged bases, and any base class that fails spike B, realize as a standalone in-memory PDF per group. |
| PDFium memory per engine is unbudgeted (feasibility) | An engine byte cap set by slice 2 (its notes: 96 MB, 16 MB an engine), plus the gate for engines after visiting 200 groups. |
| `keep_only` and `take_out` need a full mutable lopdf graph, and Rewrite time and memory are unbudgeted (feasibility) | The Rewrite gate: at most 1.2× today's time and 1.5× its RSS. Proposal 37: `keep_only` is ported to a symbolic graph only if slice 9's gate fails. |
| No CI or test-time budget (feasibility) | The "Tests, fixtures and gates" table: default, corpus (skipped without it), ignored (run per slice and before merge), nightly fuzzing, and manual Acrobat round trips. |

### Where the plan departs from the merged design

The six-lens review and the revised design came between the merge and
the plan. "The adversarial review" below records why each change was
made, and the Decisions entries cite it. Where the merge and the plan
differ, the plan stands:
- **Stale bases:** value validation with `Validate` and `Relower`
  (pinned ids), not the merge's "no re-lowering" key rule (proposal 13;
  D20).
- **M12 hooks:** dropped from M10.5 (proposal 38; D49).
- **Object streams:** M10.5 writes none, and `keep_only` keeps its lopdf
  graph (proposal 37; D40).
- **The users index:** moved to M13 (D45).
- **Object roles for M12:** a content-based classifier, not `ObjKey`
  kinds (D16).
- **The removal check's old side:** the saved file itself (D31).
- **Unencodable text:** `'?'` in what's drawn for marks and generators
  alike, so emission stays total (proposal 22; D57). The merge had
  generators refuse it.
- **Live page tokens:** reopened as a question for Greg, with a measured
  gate and a frozen fallback (proposal 8; D58).
- **Agent entry points:** out of M10.5 (proposal 36, §14.4; D68).
- **Overlay group size:** decided by slice 2's spike. Its notes record
  128 pages, scoped, against the merge's starting point of 32 (D51).
- **Size and sequencing:** 20 slices of about 68 points, with M10
  resuming only after all of them (proposal 11; D77, D78). The merge had
  18 slices of about 46 points, with M10 resuming after slice 14.
