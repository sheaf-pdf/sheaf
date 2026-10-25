# M10.5: Decisions

Part of [the M10.5 decisions](README.md).


Each entry is one fork the planning session settled: the question; what
the plan says, with its § and proposal number; what else was considered
and why it lost; which of the five designs it came from; and what the
adversarial review did to it. The plan is authoritative for what was
decided. Where its final text differs from an earlier artifact, the
entry says what the earlier one had ("the log had …"), and the plan's
version stands. Slice notes are cited where a slice settled something
the plan left to measurement.

How to read the references:
- **The log** is the synthesis decision log (55 entries). "Log F13" is
  the survey brief's fork number that the log used.
- **The lenses** are the three judges: correctness and fit to the
  future; cleanliness, simplicity and deletion; feasibility, performance
  and delivery.
- **The review** is the six-lens adversarial review: code truth, history
  soundness, PDF correctness, future fit, delivery simplicity and
  consumers. Its findings are cited as `lens/finding`.
- **Coverage.** The log's 55 entries are D1–D5, D7–D18, D20–D22,
  D26–D27, D30–D32, D36, D39–D42, D44–D48, D51–D60, D62–D66, D68–D69,
  D73–D74 and D77. The rest are forks the review added or reshaped, or
  questions the log left open.
- **Proposals.** Greg's decisions while planning are 1–7. Proposals 8–10
  are open for Greg. Proposals 11, 13 and 18–40 are the revised design's
  25 settled decisions. Proposals 12 and 14–17 were added by the plan
  writer: architecture choices the review never listed as decisions,
  put on the table for kickoff.

### Model and identity

#### D1. Which design is the base?

- **Plan:** model-definition, with grafts: from risk-first, checked
  replay, the removal backstop, the registry rule and `Transform`; from
  minimal-delivery, one primitive, pure effects, JSON ops, the Store and
  the delivery discipline; from consumer-first, Tx-as-plan, one
  ChangeSet and automatic read sets; from object-delta, the early
  encrypted spike and "Append anyway".
- **Rejected:** risk-first as the base: best on correctness, but 18
  slices of speculative tables. minimal-delivery: cleanest, but it
  defers encryption, leaving two of the brief's musts unmet (encryption
  kept, and recovery for encrypted files: its fatal flaw), and freezes
  page tokens. consumer-first and object-delta each had flaws a lens
  judged fatal (D10, D26, D51, D77).
- **From:** all five. The lenses ranked model-definition 2nd, 2nd and
  1st, the only design near the top on all three.
- **Review:** the base stands; the grafted M12 hooks were cut (D49).

#### D2. Is page content part of the model, and in what shape? (log F4)

- **Plan:** yes (decision 5). `PageDef` is base + copy + `ContentStack {
  filters, under, over }`, with static layers and filters; parametric
  content lives in document-level `Generator`s. A stack holds
  definitions, never bytes (§3.3–3.5; proposal 14).
- **Rejected:** a lazily parsed item list. Per-page decor layers with
  frozen tokens (minimal-delivery; the cleanliness lens's pick, but it
  rested on frozen tokens, and D58 chose live ones). Object-level
  content stacks (object-delta).
- **Why:** additions never decode the base, so Append stays possible; a
  watermark is O(1); history and the planner can tell a crop from a
  watermark from a removal.
- **From:** model-definition, risk-first.
- **Review:** added the `StripLegacy` filter, `FormRef::Pinned` (D56),
  pagination selection by origin (D61) and a corrected q/Q isolation
  count. M11's rewrite semantics are recorded in §20.

#### D3. What are the key kinds?

- **Plan:** 11 kinds: Order, Page, Geom, PageAnnots, Annot, Field (per
  node), Form, Bookmark (per item), OutlineRoots, Labels, Generator.
  Geom and PageAnnots are separate from Page (§3.2).
- **Rejected:** Geom inside `PageDef`, read at (Key, Part) granularity
  (model-definition): needs a Part concept. A single Outline record
  (minimal-delivery): every concurrent bookmark edit conflicts
  (correctness lens). The log also listed minimal-delivery's Form record
  here, but the plan keeps `Form` as one record too, with field nodes
  keyed per item, as minimal-delivery had them.
- **Why:** a crop, rotate or new mark during OCR doesn't conflict with
  the OCR flush, and a plugin's bookmark edits don't conflict with the
  user's.
- **From:** a synthesis of model-definition and minimal-delivery; the
  correctness lens's verdict on per-item keys.
- **Review:** kept. Order and OutlineRoots stay single values; work that
  only needs "insert somewhere" commits with `Relower` (D20) instead of
  finer keys or three-way merges of ordered keys, which are not planned
  (`consumers/hot-keys-conflict-storms`).

#### D4. Should keys and types for later milestones be reserved now?

- **Plan:** no: nothing ships without an M10.5 producer and consumer.
  The registry rule (an exhaustive destructure of `DocState`, or match
  on `Key`, in every cross-cutting function) makes each later addition
  compile-checked (§2; §19).
- **Rejected:** reserving Catalog, Security, Files and Resource keys
  (risk-first; the correctness lens wanted them for M11, M12 and M14):
  speculative generality. Adding them later with no guard
  (consumer-first).
- **From:** risk-first (the registry rule), model-definition (no
  unproduced variants).
- **Review:** the rule now covers every enum variant and public type,
  which removed `BatesStart::Continue`, `Actor::Agent`,
  `Actor::FormScript`, three group kinds and more
  (`delivery-simplicity/unproduced-variants`). `KeyPatch` left, since it
  held raw object numbers and had no producer; M10 slice 10 adds it with
  symbolic values (§3.3). `text_inventory` left the registry: M11's leak
  check reads the output.

#### D5. How do edits use sources and their facts? (log F11)

- **Plan:** ops name sources by id. Lowering reads immutable source
  facts (marks, form, outline) and records the resolved records, renames
  and ids in prims. `Document.forms` goes; lowering reads `Src::form()`
  (§3.1, §3.7, §6.3).
- **Rejected:** payloads keyed by source index (today's marks); a side
  table read during compute (M10 slice 4's forms); copying a `Slice`
  closure at insert (risk-first), a second backing kind (cleanliness
  lens) that parses and holds the whole closure up front (feasibility
  lens).
- **Why:** one convention for marks, fields and outline; replay is
  independent of facts; nothing is parsed eagerly.
- **From:** model-definition, consumer-first; against risk-first.
- **Review:** kept. Form identity was made explicit (D6).

#### D6. What identifies an inserted form?

- **Plan:** an explicit `FormInst { src, copy }`. `InsertPages` and
  `ReplacePages` take a page set, so one file selection is one op and
  one instance; the same bytes inserted again are another instance,
  renamed apart. Names come from the file the user picked
  (`FormDoc.names`), and duplicates inherit them (§3.7, §6.5).
- **Rejected:** identity keyed on (source, copy) with single-span
  inserts (the synthesis): it can't express both of M10 slice 4's rules,
  joining one file's runs and keeping two inserts of one file apart. A
  `Join` variant: no producer. Names from the blob's first origin: a
  copy is named after whichever file came first.
- **From:** the review (`code-truth/insert-runs-vs-form-instance`).
- **Review:** a new fork; it also moved `SourceId = BlobId` to slice 11
  (D7).

#### D7. How are blobs and sources named and kept alive? (log F35)

- **Plan:** `BlobId` is xxh3-128 plus length, with the bytes compared on
  every hit, and `SourceId = BlobId`. Records hold liveness handles
  (`BlobRef`, `Src`); the Store holds only `Weak`s (§3.1, §4).
- **Rejected:** SHA-256 or BLAKE3: a new crate for what a byte compare
  gives. Unchecked xxh3 (minimal-delivery): a crafted collision aliases
  content.
- **From:** risk-first, minimal-delivery.
- **Review:** handles instead of bare hashes, so snapshots and prims
  keep their bytes (`history-soundness/blob-liveness-by-id`, a blocker).
  `SourceId = BlobId` waits for slice 11: every insert registers a fresh
  `SourceId` today, and switching before `FormInst` would merge two
  inserts of one file into one form. Plugins name blobs only through
  asset handles.

#### D8. How are blank and image pages made? (log F37)

- **Plan:** small in-memory PDFs interned as `Memory(kind)` sources
  (blank, images, scan, library stamps, the comment summary) and
  inserted with `InsertPages`. No NewPage primitive, and nothing written
  to a cache (§3.9).
- **Rejected:** a model-native NewPage op with `Layer::Image`
  (minimal-delivery).
- **Why:** insert, copy and realize are reused unchanged.
- **From:** model-definition, consumer-first.
- **Review:** unchanged.

#### D9. Which copy of a page owns the base page object?

- **Plan:** a fixed `PageDef.copy` index, set at creation. Copy 0 always
  owns the base page object; if it is deleted, copy 1 is still written
  as a new object (§3.3).
- **Rejected:** "the first use in document order keeps the object"
  (today, minimal-delivery, consumer-first): moving a duplicate before
  its original flips ownership, changing `/P`, links and the object map
  (bug 3, save.rs:613; the log had ≈612).
- **From:** model-definition; a cleanliness-lens missing item.
- **Review:** added: a page tree that lists one page object twice opens
  as copy 0 and copy 1, with a notice
  (`pdf-correctness/copy-structparents-and-dup-kids`).

#### D10. What id spaces exist?

- **Plan:** one `AnnotId` space for marks, others' annotations, links
  and widgets; one `FieldId` space for every field node. No id is
  derived from an object number. Others' annotations read at open get
  ids from an rng seeded by the source hash (§4).
- **Rejected:** separate `WidgetId` and `NodeId` (consumer-first,
  risk-first): a sum type wherever the lists mix. Birth-address
  `EntityId`s (object-delta): they derive ids from object numbers, which
  the brief forbids, collide when one source object is used twice, and
  need moved-maps after every save (fatal, correctness lens).
- **Why:** `/Annots` order is one `Vector`, the field tree is uniform,
  and reopening the same file gives the same ids.
- **From:** model-definition, minimal-delivery, risk-first (seeded ids).
- **Review:** ids written to files are below 2^53 and written as strings
  (`pdf-correctness/block-marker-integers`). A duplicate `sheaf:<id>`
  gets a fresh id with a notice, and `AnnotRef::Nm` is `Ambiguous` on
  duplicates.

#### D11. Are there sub-page content ids in M10.5? (log F36)

- **Plan:** no. Sub-page identity stops at blocks (`/SheafBlock`,
  `/SheafGen`, `MarkedSel`); descriptors arrive with M11's interpreter
  (§4).
- **Rejected:** `ContentRef` or `Descriptor` types now (risk-first,
  consumer-first): no M10.5 consumer, and adding them later is purely
  additive.
- **From:** model-definition, minimal-delivery.
- **Review:** unchanged.

#### D12. Which coordinate frame do page-anchored data use? (log F8)

- **Plan:** PageSpace, the user space before `/Rotate`, for everything
  anchored to a page, Op JSON and the plugin API included. Rotation is
  absolute in `Geom`, and the render job carries the delta (§5).
- **Rejected:** displayed space with transforms on box edits (today). A
  view frame for plugins (minimal-delivery): one `Op` type with two
  meanings. Keeping the rotation delta (consumer-first): a convention
  that existed only for rebasing.
- **Why:** crop and rotate never rewrite a stored coordinate, so
  `Rebase.shift` and the widget and bookmark drift go.
- **From:** model-definition, risk-first, object-delta, consumer-first
  (the frame only).
- **Review:** specified the edges: the clipboard, `Delta::View` moves,
  view matrices for plugins, and OCR's render-time transform
  (`consumers/pagespace-at-the-edges`). Overlays keep the base
  `/Rotate`, so nothing turns twice.

#### D13. How do sheaf marks survive other tools' edits, and v1 marks?

- **Plan:** `/SheafAnnot` v2 holds PageSpace JSON plus a canonical
  digest of the standard keys sheaf wrote; a mismatch opens the mark as
  Foreign (visible, deletable, adoptable). v1 is converted on read,
  geometry from the standard keys (§3.6; proposal 21).
- **Rejected:** trusting the JSON (today, and all five designs): sheaf
  silently reverts Acrobat's edits to its marks (a correctness-lens
  finding). Opening v1 as foreign: conversion is about 30 lines, within
  decision 3's "nearly free".
- **From:** new; the conversion from object-delta, risk-first and
  minimal-delivery.
- **Review:** the digest became canonical (1/64 pt, NFC text), so a
  re-save by Acrobat or Preview keeps a mark sheaf's. Others' annotation
  text can't be edited in M10.5, and a changed `/Contents` drops `/RC`.
  Slice 5 snaps written numbers to the digest's grid (slice notes).

#### D14. How does a document open? (log F10)

- **Plan:** one open builds the complete root on a job thread. Until it
  finishes, edit and plugin commands are grayed out, `commit` returns
  `NotReady`, plugin calls queue, and `opened` fires after. Only
  source-immutable data stays lazy, and open never lexes content (§14.2;
  proposal 28).
- **Rejected:** a lazy base-plus-overrides layer (object-delta): a
  two-layer merge in every reader. Today's `set_initial_*`: six loaders
  drop facts that arrive after the first edit (bug 10).
- **From:** model-definition, consumer-first, risk-first,
  minimal-delivery.
- **Review:** dropped the log's fallback of lazy per-page `/Annots` for
  files over the 1 s budget: a slow file just waits longer, with
  progress (`consumers/loading-gaps`).

#### D15. How are sources read, and what type are reals? (log F18)

- **Plan:** `LazyPdf`, a patch to vendored lopdf over
  `Reader::get_object` and its object-stream cache, with per-object
  decryption (stream-dict strings included), one `Limits`, and an eager
  fallback (`base_sound == false`, never appended to) when the xref is
  rebuilt. `Object::Real` becomes `f64` (§3.1).
- **Rejected:** one eager `Document` per source: realizing a page or
  appending must cost milliseconds at 7,000 pages. lopdf's
  `IncrementalDocument`. Keeping `f32`: untouched dict reals wouldn't
  survive re-serialization.
- **From:** model-definition, risk-first; the code checked at
  reader.rs:364 and object.rs:83.
- **Review:** added `/EFF`, the header offset, and a positional-read
  file handle used only by the removal check.

### Primitives and history

#### D16. Are primitives model-level or PDF-object level? (log F1)

- **Plan:** model-level, recorded in history. Object-level emission
  (`ObjKey` → `SymObject`) exists only inside the materializer and
  realizer and is never recorded (§2 rules 1 and 3, §11.3; proposal 12).
- **Rejected:** PDF-object prims in history (object-delta): object
  numbers enter replay and force birth-address ids, and edits can't run
  before parsing. A two-tier design that records object patches.
- **Why:** edits work before anything is parsed, and no-op checks, dirty
  state and conflicts are `ptr_eq` on ids.
- **From:** model-definition, minimal-delivery, risk-first,
  consumer-first; against object-delta.
- **Review:** the core stands. The log's "the `ObjKey` kind serves as
  the object's role for M12" was narrowed: M12's "changes since signing"
  will use a content-based role classifier over later revisions, and the
  `ObjKey` mapping is only a test oracle (deferred to M12, §20).

#### D17. How many primitives, and of what shape?

- **Plan:** one, `Put { key, before, after }`, over 11 key kinds.
  Ordered collections are values, not splices (§6.2; proposal 12).
- **Rejected:** Order plus Put (model-definition; the feasibility lens's
  pick): a second arm in every consumer. 17 to 21 typed prims
  (consumer-first, risk-first): arms with no consumer benefit. A `Set`
  without a before-value (minimal-delivery): see D18.
- **Why:** every consumer handles one case, and a new collection never
  adds a primitive. Before-values are the parent's Arcs, so they cost
  nothing.
- **From:** minimal-delivery (one Set), model-definition and risk-first
  (before-values).
- **Review:** prims are coalesced per key at commit and amend, so a
  change that nets to nothing pushes no version
  (`history-soundness/net-identity-and-unchanged`).

#### D18. What does a step store, and how does replay work? (log F2)

- **Plan:** the resolved op, prims with before-values, effects, the
  describe line against the pre-op state, and cascades. Replay applies
  prims, asserts before-values and never re-lowers; a `Mismatch` is
  shown as "History is damaged at version N". Snapshots stay, so undo is
  O(1) (§8.1–8.2; proposal 12).
- **Rejected:** intent plus seed with re-lowering (today): drifts when
  an unrecorded input changes (bug 6). After-only deltas
  (minimal-delivery, consumer-first; the cleanliness lens's pick):
  replay can't fail, but can't be checked. Prims alone.
- **Why:** undo can't silently diverge, lowering can grow without
  endangering replay, and effects come from the step even after its
  parent is trimmed. The plan cites ProseMirror, Slate and Lexical.
- **From:** risk-first, model-definition.
- **Review:** root versions drop their prims (D24), and lines are
  recomputed when a commit merges onto a moved cursor.

#### D19. Does a version id name one fixed state?

- **Plan:** yes. An amend replaces the version with a new id under the
  same parent, and the old id stops resolving, so the token `{ instance,
  version }` alone says whether anything changed (§8.1, §8.4; proposal
  13).
- **Rejected:** amending in place under the same id (today, bug 7, and
  the synthesis): every check keyed on a version id can be fooled. A
  crop dialog committed during OCR would silently drop the layers OCR
  added since it opened, a stale calculation could commit, and OCR
  amending one version for hours would never trigger autosave. Three
  review lenses found it independently, two of them as a blocker. A rev
  counter or amend epoch beside the id (what the findings proposed): a
  new id per amend keeps ids naming immutable states, so the base ==
  cursor fast path stays exact.
- **From:** the review
  (`history-soundness/amend-breaks-version-identity`, with duplicates
  from the future-fit and consumers lenses).
- **Review:** a new fork.

#### D20. What happens when a transaction's base is stale? (log F3, F24)

- **Plan:** commits validate by value. A stale commit checks the
  caller's decision reads, then either `Validate` (lowering reads and
  overwritten values checked, prims applied, cascade and uniqueness
  rules re-run) or `Relower` (ops lowered again on the cursor with
  pinned ids; a Conflict if handed-out ids would change). §8.5 gives
  each caller its policy (§8.5; proposal 13).
- **The log had:** `IfDisjoint` (apply unchanged prims if no key read or
  written was touched since the base), `Refuse` for the agent,
  `BaseGone` for a pruned base, and no re-lowering.
- **Rejected in planning:** re-lowering (model-definition,
  consumer-first, object-delta): it changes ids already returned. Strict
  refusal. Silent application (today).
- **Review:** replaced the rule. Path-key unions were inexact under
  amends (D19), missed phantoms such as a generator scoped to a page a
  stale delete removes, and refused unrelated work on hot keys.
  `Relower` brings re-lowering back with pinned ids, so the log's
  objection becomes a Conflict, not a renumbering. Three-way merges were
  rejected as unnecessary.

#### D21. What does a transaction record as read?

- **Plan:** two kinds, both by value: lowering reads (accessors,
  cascades included) and decision reads (`Tx::read`, `Tx::reader()`,
  host-resolved payloads such as calculations and OCR words, and
  `read_render` for anything derived from pixels or text) (§6.3).
- **Rejected:** hand-declared reads (minimal-delivery's planners):
  disjointness needs complete reads. No reads for host payloads (all
  five designs): a concurrent change to a calculation input goes
  unnoticed (correctness lens).
- **From:** consumer-first (automatic recording); new (host payload
  reads).
- **Review:** WIT reads and UI macros record decision reads. Analysis
  renders use `ViewInputs::content_only()`, so OCR reads `Page(p)`
  alone: a crop during OCR never conflicts, and an unsaved header never
  enters an OCR layer
  (`history-soundness/render-derived-payload-reads`).

#### D22. How do streaming jobs and gestures map to undo steps?

- **Plan:** groups for streaming producers: `GroupKind` is `Ocr` and
  `PluginCall`, and M10 slice 11 adds `Typing`. Dialogs and drags
  preview with `Tx::preview` and commit once (§8.4, §8.6).
- **Rejected:** amending history on every mouse move: history would see
  intermediate gesture states.
- **From:** consumer-first, model-definition.
- **Review:** the log's Typing, Gesture and Slider kinds went for lack
  of an M10.5 producer. `GroupEnded` holds exactly when the group's
  latest version isn't an ancestor-or-self of the cursor
  (`history-soundness/groupended-contradiction`). User edits in between
  fragment a group, undone one fragment at a time or all at once by
  `revert_group`. Previews lower in the background and re-lower on every
  ChangeSet.

#### D23. How does a failed plugin call or group roll back?

- **Plan:** `rollback_group` truncates when the group's only version is
  the childless, unsaved, unsealed cursor. Otherwise it reverts with
  inverse prims validated by value; on a conflict it keeps the work and
  says so (§8.4; proposal 19).
- **Rejected:** rollback as a jump to the pre-group version (the
  synthesis): it drops edits the user made in between, and can cross a
  save (`history-soundness/rollback-group-partial-and-saves`).
- **From:** the review.
- **Review:** a new fork.

#### D24. How does history stay within its memory budget?

- **Plan:** trimming snapshots frees snapshot bytes; if step and blob
  bytes still exceed the budget, versions are pruned (off-line branches
  first, then the oldest on the line). A detached saved anchor lets the
  root pass the saved version. A version that becomes the root drops its
  prims, and sealed and saved versions are never pruned (§8.7).
- **Rejected:** the synthesis's accounting, which pinned saved versions
  and couldn't free step bytes, so 200 re-OCRs of a 1,000-page scan
  (about 2.4 GB of layers) couldn't stay within budget
  (`history-soundness/memory-budget-unenforceable`).
- **From:** the review. The cleanliness lens had noted that no design
  simplified the history policy.
- **Review:** a new fork, placed in slice 12.

#### D25. What do deleted pages do to fields and labels?

- **Plan:** a field whose last modeled widget is deleted is removed,
  inverting the model test `form_fields_follow_their_pages`
  (model.rs:3055). A label range whose first page is deleted re-anchors
  to the next surviving page of its range (§6.5; proposal 25).
- **Rejected:** today's behavior: such fields stay in the model though
  `keep_only` drops them from the file, and label anchors aren't
  validated at all.
- **Why:** the first makes the model match the file; the second keeps
  the labels the user set.
- **From:** a synthesis open question (recommendation: accept both).
- **Review:** settled as recommended. `widget.field` skips terminals
  with widgets on page templates
  (`code-truth/reachability-vs-templates`).

### Saves and the planner

#### D26. What does a save do to the model? (log F15)

- **Plan:** saves are outputs. A `SaveRecord` is attached to the saved
  version, nothing is re-expressed, edits during a save stay dirty, and
  one emitter feeds rewrite, append, recovery and overlay (§9.1;
  proposal 15).
- **Rejected:** rebasing the saved version in place (consumer-first,
  object-delta): it breaks replay of versions made during the save.
  Rebasing the cursor (today): an edit made during a save is lost or
  marked saved (bug 1).
- **Why:** deletes `rebase_to_saved`, rebinding, the reopen and the tile
  drops.
- **From:** model-definition, risk-first, minimal-delivery.
- **Review:** the version is sealed until the save ends, so an OCR flush
  can't amend it mid-save
  (`history-soundness/amend-during-inflight-save`, a blocker). Save As
  became a Save (D34).

#### D27. How does an Append survive undo across a pruning Rewrite?

- **Plan:** `SaveRecord.present` records which primary objects the
  written file still has; an emission that references an absent one
  forces a Rewrite (`BaseObjectsMissing`). One static ownership table
  per root object: owned keys are always emitted from the target state,
  and unowned keys come from `SaveRecord.roots`, the values actually
  written (§9.1).
- **Rejected:** an identity map for base objects (model-definition,
  minimal-delivery, risk-first): delete pages, Rewrite, undo, save would
  Append references to freed objects, a broken file (the feasibility
  lens judged it fatal in all three).
- **From:** new, from missing items of the feasibility and cleanliness
  lenses.
- **Review:** added the ownership table, so set, Append, undo, Append
  and reopen equals the original for every owned key
  (`future-fit/root-key-ownership-and-undo`).

#### D28. What does an Append append to?

- **Plan:** the saved file on disk, never a `Source`'s bytes. The
  section is built in memory and removal-checked, the file stamp is
  re-checked, a journal is written for torn tails, and the section is
  appended in place (§11.7).
- **Rejected:** the synthesis's "the original bytes verbatim, then the
  section": read naively, a second Append rewrites the opened bytes and
  loses the first. A temp file and rename per Append: it copies the
  whole base, about 500 MB on a large scan, where the gate allows 50 ms.
- **From:** the review
  (`history-soundness/append-base-bytes-unspecified`,
  `pdf-correctness/append-durability-cost`).
- **Review:** a new fork.

#### D29. What does an Append emit?

- **Plan:** the closure of the keys changed since the save under
  `emit_deps`, a registry-rule function with a dependency table (labels
  on Order, links by whether their target is written, live-token pages,
  Geom to blocks, Form to inheriting widgets' appearances). A
  default-run property test checks that every Append reopens to the same
  facts as a fresh Rewrite (§7.2).
- **Rejected:** emitting only the changed keys' own objects (the
  synthesis): dependents such as `/PageLabels` after a reorder are
  missed.
- **From:** the review
  (`history-soundness/append-emit-scope-not-dependency-closed`).
- **Review:** a new fork.

#### D30. Which changes force a full rewrite? (log F13)

- **Plan:** strict, judged by content against the saved value: `REMOVES`
  and `REPLACES_TEXT` force a Rewrite; geometry, style and live redraws
  append. An object-level removal check vetoes any Append that would
  leave saved text readable (§7.1, §9.2, §11.9; proposal 9, open for
  Greg).
- **Rejected:** by id (today): a crop rewrites while replaced text
  appends. Sensitivity tiers. No backstop (the cleanliness lens: two
  mechanisms that must agree): two lenses wanted it, since Appends are
  small and a misclassification then only costs speed.
- **Why:** extends M9's "deleting has to delete" to every replaced text.
- **From:** risk-first (the backstop), model-definition, consumer-first,
  minimal-delivery.
- **Review:** made an open question with its costs (a note edit after a
  save rewrites, about 0.5 s on the synthetic 7k; signed files ask
  more). On a signed document a veto asks instead of rewriting.

#### D31. What does the removal check compare against?

- **Plan:** the saved file itself, read lazily over positional reads.
  Derived objects (mark and widget appearances, generator, OCR and
  isolation blocks, `PageRes`) are judged on their record inputs; source
  objects on decoded NFC word multisets, where an unknown encoding
  counts as differing. Embedded fonts keep their GIDs (§11.9).
- **The log had:** `emit(saved_state, key)` when the record says sheaf
  wrote that key, and the source object otherwise.
- **Rejected:** always `emit(saved_state)` (risk-first): for a mark
  whose appearance came from an older build or another tool, regenerated
  bytes aren't what's in the file (correctness lens). Comparing encoded
  operands: a font or subset change looks like a removal.
- **From:** risk-first, with the correctness lens's fix.
- **Review:** changed as above
  (`pdf-correctness/removal-check-old-side`,
  `history-soundness/removal-check-encoding-and-marker`).

#### D32. May a regenerated page number append?

- **Plan:** yes. Re-laying out a generator under an unchanged recipe is
  `LIVE_REDRAW`, and appends. The check allows exactly those blocks
  (`Emission.allowed_drops`), verified by the recipe digest `/R` (§3.5,
  §7.1; proposal 8).
- **Rejected:** a strict Rewrite on any changed generated text
  (risk-first, model-definition): every reorder after a save would
  rewrite 7,000 pages (feasibility lens).
- **Why:** a regenerated page number leaks nothing.
- **From:** the correctness lens's verdict on F13; a new mechanism.
- **Review:** the block marker carries separate layout and recipe
  digests, written as strings, and `Generator::needs()` limits which
  pages a reorder re-lays out. Slice 17's gate (an Append of at most 8
  MB and 1 s, numbers the plan writer proposed) measures the cost and
  decides proposal 8.

#### D33. How are signed and certified files saved?

- **Plan:** `SigImpact` per signature, against the path's `SigClass` set
  (FILL, ANNOT, OTHER) with DocMDP, FieldMDP and UR3 as allow masks. A
  Rewrite or a disallowed Append asks, offering "Append anyway" for
  removals, replaced text, form transforms and removal-check vetoes,
  never for an unsound base, missing objects, a changed file or no
  record (§9.2–9.3).
- **Rejected:** strict removal with no Append choice: deleting a comment
  on a P3-certified file would break its signature (correctness lens,
  grafting object-delta). M10 slice 5 already ships the choice.
- **From:** object-delta, risk-first (the impact table), M10 slice 5.
- **Review:** `SigClass` became a set, so M12's SIGN and LTV fit.
  "Append anyway" covers form transforms, and transforms touching the
  Catalog or AcroForm count as OTHER. Locked-field refusal waits for M10
  slice 9. The mapping stays conservative until an Acrobat check (Asks).

#### D34. What does Save As do to a signed document?

- **Plan:** it copies the saved file (a reflink, or a verified stream
  copy) to the new path and appends, so signatures survive; Save As of
  an unsigned document rewrites (§9.1; proposal 32). The CLI's `-o
  --append` does the same.
- **Rejected:** Save As as an export Rewrite (the synthesis): it breaks
  every signature. Acrobat copies and appends.
- **From:** the review (`future-fit/save-as-breaks-signatures`).
- **Review:** a new fork.

#### D35. What if the file changed on disk?

- **Plan:** always ask: Save As…, Overwrite or Cancel. The `FileStamp`
  (length, mtime and two content hashes) is re-checked just before the
  rename or the in-place append (§9.2, §11.7; proposal 33).
- **Rejected:** today's length-only check, which silently rewrites over
  the other program's changes (bug 2).
- **From:** the review
  (`history-soundness/file-changed-on-disk-silently-overwritten`).
- **Review:** a new fork. Slice 7 landed the stamp check and removed the
  length-only check, two slices early (slice notes).

#### D36. Where do save-time form transforms live? (log F23)

- **Plan:** `forms/rules.rs` becomes `rules::plan`, a pure plan-time
  function; the emitter applies its transforms (`DropXfa`, `DropUr3`,
  `FixSigFlags`, `FillAppearances`) on every path, and
  `SaveRecord.transforms` and `roots` remember what the file already
  lost (§11.8).
- **Rejected:** recording the transforms as history edits: undo stops
  being exact. Porting `take_out`, which mutates the written graph after
  the fact: the rules move to the plan stage.
- **From:** minimal-delivery, model-definition.
- **Review:** `FixSigFlags` decides from the target's signed fields,
  inserted forms included; recovery sections and exports apply the
  transforms; `DropXfa` rides an Append only when the datasets hold no
  replaced value.

#### D37. Is UR3 dropped on every save?

- **Plan:** it is kept on Appends whose changes lie within UR3's
  `/TransformParams` (form fill-in allows FILL, `/Annots` allows ANNOT),
  and dropped on every Rewrite (§11.8; proposal 34).
- **Rejected:** dropping it on every save that touches the form (the
  synthesis): Reader validates later revisions against
  `/TransformParams`, so a permitted fill or comment would lose Reader's
  rights for nothing.
- **From:** the review (`pdf-correctness/ur3-drop-on-append`).
- **Review:** a new fork.

#### D38. What happens to signed fields brought in from another file?

- **Plan:** they are kept with their values and the
  `SignatureNotValidHere` notice, as M10 slice 5 ships; M10 slice 15's
  flatten can flatten them (§6.5).
- **Rejected:** turning them into unsigned fields
  (`future-fit/incoming-signed-fields-and-templates`): it contradicts
  slice 5's tested behavior
  (`signed_fields_brought_in_are_kept_and_said_so`). This was the
  review's only rejected finding. Template spawning went to M10 slices
  19–22.

#### D39. What is the recovery format? (log F14)

- **Plan:** sidecar v2: an Append section from the saved file to the
  cursor, from the same serializer, or a Full write when base objects
  are missing, the base is unsound or there is no record. It is
  encrypted for encrypted files, carries the base's length and stamp,
  and is restored in memory (§11.10).
- **Rejected:** a semantic edit log with persisted blobs, or a hybrid: a
  second format.
- **Why:** self-contained, covers every state, and costs O(edits).
- **From:** all five.
- **Review:** sections apply form transforms but not strategy rules. A
  removals flag makes the restored document's first save a Rewrite (or
  the signed-file ask), so a crash never leaves removed text in an
  earlier revision unasked. `record_save` rewrites or deletes the
  sidecar synchronously
  (`history-soundness/recovery-sidecar-lifecycle`).

#### D40. How are objects numbered and copied? (log F17)

- **Plan:** a Rewrite keeps primary numbers, and exports use Dense
  numbering. An object is raw-copied only under five conditions (stable
  numbering, the primary source, the same crypt key and `/ID[0]`,
  untouched by emit and `keep_only`, not in an object stream). M10.5
  writes no object streams (§11.6; proposal 37).
- **The log had:** members repacked into new object streams on
  xref-stream bases.
- **Rejected:** renumbering with a map (today). Raw-copying
  object-stream containers: stale copies of replaced members.
- **From:** model-definition (raw spans), risk-first (Dense exports);
  packing was new.
- **Review:** packing went to M14: as specified it encrypted strings in
  object streams twice
  (`pdf-correctness/objstm-packing-double-encrypts`, a blocker), and
  nothing needs it yet. `keep_only` is ported only if slice 9's Rewrite
  gate fails.

#### D41. Which writer serializes PDF? (log F19)

- **Plan:** sheaf's own (from incremental.rs's writer), with append and
  full framings, the base's xref kind, a real `/Prev`, per-object
  encryption and fsync (§11.7).
- **Rejected:** lopdf's `IncrementalDocument`: no control over xref
  kind, `/Prev`, generations or offsets.
- **From:** all five.
- **Review:** in-place appends with a journal (D28); offsets relative to
  the header; one xref subsection with linked free entries;
  deterministic AES IVs from HMAC-SHA256 of the key, object and
  plaintext; the PDF version raised as features need, through Catalog
  `/Version` on an Append. M12's offsets went (D49).

#### D42. When does encryption preservation land?

- **Plan:** in M10.5's serializer and savers, slices 7, 9 and 10:
  rewrite, append, recovery and overlays, all per object. Loads stop
  stripping `/Encrypt`. M11 slice 1 shrinks to notices, Properties,
  owner-only files, lopdf #584, the certificate refusal and temp cleanup
  (§11.7, §20; proposal 23).
- **Rejected:** deferring to M11 behind a hook (minimal-delivery):
  encryption kept and recovery for encrypted files are musts in the
  brief, and leaving them unmet was that design's fatal flaw. Patching
  the old writers first (model-definition): throwaway work in writers
  about to be deleted (feasibility lens).
- **From:** consumer-first, risk-first; the feasibility lens.
- **Review:** settled as recommended. The reorder put the serializer at
  slice 7, so the old writers prove it, encryption included, before they
  are deleted.

#### D43. Do exported copies keep the source's encryption?

- **Plan:** yes, by default, for Extract, Split, Export Flattened and
  Clean Copy. Print and page images stay unencrypted bytes in memory;
  Save As always keeps it. There is no per-dialog checkbox in M10.5;
  M11's security UI adds the choice (§11.7, §12.3; proposal 10, open for
  Greg).
- **Rejected:** today's unencrypted exports: a password-protected client
  file shouldn't produce unprotected copies silently. A checkbox in each
  export dialog (the log's recommendation): dropped in triage.
- **From:** a synthesis open question; the review's triage
  (`delivery-simplicity/open-questions-triage`).
- **Review:** kept open, without the checkbox. Slice 7 implemented the
  recommendation under the standing instruction (slice notes).

#### D44. How is the page tree written?

- **Plan:** an Append keeps the saved tree's shape and rewrites changed
  `/Pages` nodes in place. Inserted pages join their predecessor's node,
  and a node splits only past max(128, its size at open). A Rewrite
  writes a balanced tree with fanout 32 (§11.6).
- **Rejected:** flattening on the first structural save
  (minimal-delivery): every page's `/Parent` is rewritten once. Always
  rewriting a flat tree (today).
- **Why:** a reorder in a flat 7,000-kid tree writes one object, and a
  crop writes no tree objects.
- **From:** consumer-first, model-definition.
- **Review:** a page placed under a new parent gets all four inheritable
  keys written explicitly, `/Rotate 0` included
  (`pdf-correctness/inheritance-on-reparent`). A balanced re-tree on
  Append waits until corpus tests ask for one.

#### D45. How are shared objects edited? (log F34)

- **Plan:** each use forks, by construction: the writer never writes a
  source object's number with different content, except model-owned
  objects and `keep_only`'s pruning. A transformed stream is a new key
  memoized by (object, transform). `verify.rs` enforces this as a
  post-condition (§11.5, §11.9).
- **The log had:** also `Source::users` and an in-model users index,
  with apply refusing an out-of-scope shared write.
- **Rejected:** mutating once (today's `stamps::strip`): the most common
  cross-page corruption in prior art.
- **From:** minimal-delivery, object-delta (the users index),
  risk-first.
- **Review:** the users index and the apply-time check went for lack of
  a producer, and the post-condition replaced them
  (`delivery-simplicity/shared-object-machinery`). They arrive with the
  first "all uses" op (M13, M14).

#### D46. How do exports run? (log F16)

- **Plan:** `export::run`, the only writer besides Save: preflight
  gates, a resolve stage, an `ExportTx` with scratch ids and no history,
  `rules::plan` as a Rewrite, materialize, verify, and an atomic write
  or bytes (§12).
- **Rejected:** a discarded history branch. Bespoke subset and flatten
  passes (today).
- **Why:** exports stay out of the History panel and plugin events, are
  `Send`, and share one lowering and rule set.
- **From:** model-definition, risk-first, minimal-delivery.
- **Review:** Save As left the export table and became a Save, and every
  export runs the form rules and asks before dropping XFA that holds
  values (`code-truth/export-skips-form-rules`); large derived bodies
  are produced while writing, with a peak-memory gate
  (`future-fit/transforms-not-streaming`).

#### D47. How are export-only operations kept out of the binder?

- **Plan:** a separate `Transform` enum (`KeepPages`, `RemoveMarkup`,
  `RemovePrivateData`, and `Op`; M11 adds `ApplyRedactions` and
  `Sanitize`) that only `ExportTx` accepts. `Document::commit` takes a
  `Tx`, which can't hold one. Binder-capable transforms such as
  `FlattenAnnotations` are ops (§6.1, §6.7).
- **Rejected:** a runtime `Cap::Export` class (minimal-delivery,
  model-definition; the cleanliness lens's pick, for one vocabulary):
  with the type, "plugins never apply redactions" and "every way out
  asks first" are type-level facts, for about 30 lines.
- **From:** risk-first.
- **Review:** the synthesis's one `Tx` with an export mode became two
  types, `Tx` and `ExportTx`, and caps are checked before lowering
  against an actor bound by `lower_cx`
  (`consumers/tx-mode-runtime-caps-in-commit`).

#### D48. How does extracting pages treat the outline? (log F33)

- **Plan:** exports use `KeepPages { Headings }`: a bookmark whose page
  goes, but which keeps surviving descendants, becomes a heading.
  `DeletePages` keeps "promote" (§6.5, §12.3).
- **Rejected:** one rule for both.
- **Why:** each command keeps its documented behavior, as an explicit
  policy.
- **From:** all five.
- **Review:** unchanged.

#### D49. Are M12's signing hooks built now?

- **Plan:** no. Reservations, byte-range holes and returned offsets are
  out of M10.5, and revision reading stays only for forms' earlier
  values. M12's rules are recorded in §20: signing is two commits around
  one Append, and only when the plan is an Append (proposal 38).
- **Rejected:** building and testing the hooks now (risk-first; the log
  grafted them, and the correctness lens wanted them so the writer
  needn't be reopened later): no M10.5 producer.
- **From:** the review (`delivery-simplicity/m12-hooks-yagni`; the
  signing findings were deferred to M12).
- **Review:** reversed the log's graft.

#### D50. What smaller PDF rules did the review fix?

- **Plan** (fixes, with no alternatives weighed): a merged field and
  widget is one object under `ObjKey::Field` (§11.3). Isolation q/Q
  counts come from `Balance { net, min_prefix }` (§3.4). `PageRes` omits
  names used only inside dropped ranges, so an Acrobat watermark's
  resources go too (§11.5). Ids, digests and Bates numbers in markers
  are strings (§3.5). The `/SheafAnnot` digest is canonical (D13). Also
  the header offset, `/EFF`, the PDF version, the xref shape and AES IVs
  (D41).
- **From:** the review's PDF-correctness lens.

### Rendering

#### D51. How are edited pages rendered? (log F5)

- **Plan:** in-memory append sections over the source's bytes, one per
  group of pages, from the same emitter and serializer as saves;
  per-page deltas cached by `RenderKey`. Untouched pages use the base
  engine; damaged bases realize standalone (§10.1; proposal 16).
- **Rejected:** per page (minimal-delivery; the cleanliness lens's
  pick): engine churn after document-wide edits. 64 or 256 pages
  (risk-first, consumer-first): warm caches lost for hundreds of pages
  per edit. Per-source generations (object-delta): each commit
  re-serializes every edited page. Scratch PDFs. `GenerateContent`:
  lossy, about 95 ms, mutates the document.
- **From:** model-definition (32 pages, tunable); the feasibility lens.
- **Review:** one object allocator per source, written spike criteria,
  and a spike C for box edits on the base engine.
- **Slice 2:** GROUP = 128, as scoped sections, a measured departure
  from §10.1 (in place, every engine on the heavy fixture loaded all
  7,000 fields); 6 screen engines in 96 MB, 2 background in 32 MB. Spike
  C failed, so crops use overlays (D62).

#### D52. How are encrypted sources rendered? (log F20)

- **Plan:** overlay sections are encrypted with the source key; if
  PDFium rejects that, the fallback for that source is a decrypted
  in-memory image. Nothing reaches disk either way (§10.1).
- **Rejected:** always decrypting in memory (minimal-delivery): a second
  path, O(file) on the first edit. Untested plaintext sections.
- **Why:** the same code path as saves, and spiking it early avoids
  replanning mid-milestone.
- **From:** model-definition, object-delta (the early spike),
  minimal-delivery (the fallback).
- **Review:** unchanged. Slice 2's spike A passed: no pixel differed for
  RC4-128, AES-128 or AES-256, and 50 of 50 encrypted corpus files
  passed, so no fallback is needed.

#### D53. Who draws sheaf's marks on screen in M10.5? (log F21)

- **Plan:** GPUI, as today, with `MarkDraw::ByHost` telling the realizer
  to leave them out. M10 slice 12 switches to PDFium drawing their
  `/AP`, after slice 7's fallback font, and deletes the GPUI painter,
  `stamp_preview.rs` and the tape pictures (§10.4; §2 rule 10).
- **Rejected:** switching in M10.5 (consumer-first, and risk-first
  behind a cut line): commit-to-tile latency and pixel parity on the
  critical path, and `'?'` or refusals for non-Latin text until the font
  exists.
- **From:** model-definition, minimal-delivery, object-delta;
  risk-first's pairing with the font.
- **Review:** unchanged.

#### D54. Where does page text come from?

- **Plan:** the base engine for untouched pages and the realization for
  edited ones; field display text merges in with M10 slice 14 (§10.5).
- **Rejected:** characters computed from layout (minimal-delivery): they
  can drift from what PDFium extracts (feasibility lens).
- **Why:** the screen, the file and the text agree by construction, and
  inserted, cropped, duplicated and unsaved-OCR pages become searchable
  (M11 bug 6).
- **From:** model-definition, consumer-first, risk-first.
- **Review:** plugin text leaves out field display text until 0.5
  (`consumers/r98-values-leak`).

### Content toolkit

#### D55. How much of M11's interpreter moves forward? (log F9)

- **Plan:** only the lexical layer (raw-span tokens, opaque inline
  images, marked ranges across streams, q balance, a byte-preserving
  splice, names used, the text bag and block classification), plus
  `ContentWriter` and the `Font` service. The semantic interpreter stays
  in M11 (§13; proposal 17).
- **Rejected:** the semantic interpreter now: not needed to strip and
  remove faithfully. hayro-syntax: no separate reason is recorded, and
  hayro stays a test oracle only. Keeping lopdf's `Content` round trip:
  f32 numbers and broken inline images.
- **Why:** enough to strip, remove and feed the removal check, and M11's
  interpreter consumes the same token spans, so nothing is thrown away.
- **From:** all five agree; model-definition, minimal-delivery.
- **Review:** inline images follow PDFium's rules
  (`pdf-correctness/inline-image-lexing`). A splice is accepted only
  when balanced on its own or inside one text object with no text-state
  operators (`pdf-correctness/splice-range-safety`). Classification by
  origin was added (D61).

#### D56. How are appearances produced? (log F22)

- **Plan:** derived at emit time from recorded inputs (display text
  included) by a pure `appearance()` memoized by input digest. The font
  is an explicit input; M10 slice 7's fallback is late-bound per output
  as a GID-preserving subset. A flattened appearance is pinned by its
  inputs (§11.3, §3.4).
- **Rejected:** storing appearance bytes in history.
- **Why:** a small history, and one generator for screen and file.
- **From:** all five.
- **Review:** pinning captures inputs, not bytes, so a flattened
  appearance never depends on a deleted annotation or a cache
  (`future-fit/formref-ap-dangles-after-flatten`). Subsets keep GIDs so
  re-subsetting never looks like a removal. PDF/A's font policy went to
  M14.

#### D57. What happens to unencodable text until M10 slice 7?

- **Plan:** one interim rule for marks and generators: `'?'` in what is
  drawn, the exact text kept in `/Contents`, specs and recipes, and a
  one-time notice. Emit stays total (§3.5, §13; proposal 22).
- **The log had:** new generators (header/footer, watermark, fields)
  refuse such text at commit; marks keep today's `'?'`.
- **Rejected:** refusing in marks too (model-definition,
  consumer-first): a regression in typing for a few weeks. Pulling Noto
  into M10.5 (risk-first): scope pulled forward. Generators refusing
  (the log): a later label change can make a live token unencodable, and
  the document unsaveable (`consumers/live-token-unencodable-later`).
- **From:** object-delta, minimal-delivery; the review.
- **Review:** changed to the one rule.

#### D58. Are header/footer page numbers live or frozen? (log F6)

- **Plan:** page-number, count and label tokens are live in every
  output; Bates numbers and date and file tokens are frozen at apply;
  `Scope::All` covers pages added later. If slice 17's gate fails, fall
  back to frozen plus "Update page numbers" (§3.5; proposal 8, open for
  Greg).
- **Rejected:** all frozen with Update (minimal-delivery; the
  cleanliness and feasibility lenses' pick, since it deletes the
  live-token machinery): numbers go stale, an extract prints "Page 5 of
  7000", and later pages never get the header. Live until save, frozen
  after (today): two rules, depending on the save. Live Bates: they are
  identifiers.
- **From:** model-definition, consumer-first, risk-first, object-delta;
  the correctness lens.
- **Review:** kept as the recommendation, made an open question with the
  costs on both sides.

#### D59. How do frozen Bates numbers survive save and reopen?

- **Plan:** each drawn block's stream dict carries `/SheafBlock << /K
  /Gen /G … /L … /R … /N (000123) >>`. On reopen, `Pages` scopes and the
  frozen map are rebuilt from the blocks; a covered page with nothing to
  draw gets an empty block. If another tool merged a block's stream,
  Update re-lays that page with a notice (§3.5, §11.4).
- **Rejected:** a frozen map keyed only by session `PageId`s (all five
  designs): Update and Remove can't recover the numbers after a reopen
  (a correctness-lens finding missing from all).
- **From:** new.
- **Review:** numbers and digests are strings, so a PDFium or Acrobat
  re-save can't round them (`pdf-correctness/block-marker-integers`),
  and empty blocks let scopes rebuild exactly. The log's
  `BatesStart::Continue` for M15 left M10.5 as unproduced.

#### D60. Which way up are watermarks after a rotate? (log F32)

- **Plan:** upright in the displayed frame, laid out at emit time, like
  the header/footer (§3.5; proposal 30).
- **Rejected:** fixed to the page at apply (today, minimal-delivery).
- **Why:** one rule for all generators, and the dialog's preview equals
  the result.
- **From:** model-definition, consumer-first, risk-first.
- **Review:** settled as recommended.

#### D61. Whose headers and watermarks does sheaf replace or remove?

- **Plan:** each pagination artifact is classified as sheaf's, legacy
  sheaf, Acrobat's (a Form XObject with `/PieceInfo`
  `ADBE_CompoundType`) or authored (any other `/Artifact /Pagination`,
  as Word, LibreOffice and InDesign write). Set replaces Acrobat-added
  ones of its kind; Remove never touches authored ones without
  `include_authored`; dialogs show counts (§3.4, §6.5, §13; proposal
  31).
- **Rejected:** treating every pagination artifact as removable (the
  synthesis): it removes an author's running heads unasked
  (`pdf-correctness/pagination-artifacts-overmatch`).
- **From:** the review.
- **Review:** a new fork. Slice 6 applied it to strip early, after a
  refused range failed Header & Footer on 421 of 689 corpus files (slice
  notes).

#### D62. What does crop do? (log F12)

- **Plan:** box-only `SetPageBoxes`, which appends with only page dicts
  in the section. Removing content outside the box is M11's (§5, §16;
  proposal 27).
- **Rejected:** a content-removing crop by default: it needs the
  interpreter now.
- **Why:** cheap, reversible, and what Acrobat and MuPDF do.
- **From:** all five.
- **Review:** settled as recommended. Slice 2's spike C failed (PDFium
  draws live widgets only through `FPDF_FFLDraw`, which takes no
  matrix), so crops render as overlays rather than as box edits on the
  base engine.

### Consumers and plugins

#### D63. What does the public edit API look like? (log F7)

- **Plan:** one Rust `Op` enum in a types-only `sheaf-edit` crate
  (serde, schemars, `deny_unknown_fields`). Plugins send it as JSON over
  WIT 0.4, the only world served; reads are typed WIT. A new op needs no
  WIT bump (§6.1, §14.3; proposal 18).
- **Rejected:** a WIT mirror with a parity test (model-definition,
  object-delta) or total conversions (risk-first; the correctness lens's
  pick): a second vocabulary. A tx resource with a method per op
  (consumer-first).
- **From:** minimal-delivery; two of three lenses.
- **Review:** the log's `"v": 1` became an envelope `{ "v": "1.0",
  "label"?, "ops" }` with an `edit-schema` manifest check, defaulted
  later fields and per-kind references
  (`consumers/op-json-versioning-gaps`).

#### D64. How do plugin calls deliver edits? (log F25)

- **Plan:** one Tx per call. `submit` lowers on the plugin thread and
  returns ids; `commit()` is optional and blocking; an `Ok` end commits
  with `Relower`; an `Err`, trap or cancel rolls back (D23). The call is
  one undo step unless the user edits in between (§14.3; proposal 19).
- **Rejected:** a blocking UI-thread commit per submit
  (model-definition, consumer-first, risk-first; the correctness lens's
  pick): a round trip each and a deadlock surface. Commit only at the
  end (minimal-delivery): returned ids a conflict can void.
  Fire-and-forget (today): failures drop edits silently.
- **From:** minimal-delivery, with consumer-first and risk-first.
- **Review:** the Tx is built lazily at the first read or submit. Events
  merge, never echo a plugin's own changes, and edits answering an undo
  are refused (`consumers/undo-fighting-plugins`).

#### D65. Which ops may plugins submit first? (log F26)

- **Plan:** `Cap::Plugin` ops: today's 13 WIT 0.3 edit cases, plus
  labels, replies and review states through locals, bookmark style, page
  stamps and attachments through uploaded assets, and deleting sheaf
  marks. Header/footer, OCR, inserting pages and deleting others'
  comments come later, with manifest permissions (§14.3, §6.7; Greg's
  decision 6).
- **Rejected:** exposing more now: semantic edits only at first, growing
  additively through JSON ops.
- **From:** all five.
- **Review:** a cap is computed from the op and the kinds of its
  targets, so deleting sheaf marks is `Plugin` while others' comments
  need `Approval` (`code-truth/plugin-surface-widened`). Call-scoped
  asset handles brought stamps and attachments into the first cut.

#### D66. What do plugin reads show? (log F27)

- **Plan:** content as the file will draw it now: renders and text come
  from the call's state through realizations, unsaved header and OCR
  included, with a without-markup option. Coordinates are PageSpace,
  with view matrices (§14.3).
- **Rejected:** "as the file draws it", from source pages (today): an
  inconsistent picture, with OCR special cases.
- **From:** model-definition, risk-first.
- **Review:** reads record decision reads (D21). `annotations()` never
  returns widgets, text leaves out field values, and authors are roles;
  gating values in renders waits for 0.5's fields capability
  (`consumers/r98-values-leak`).

#### D67. What may a plugin learn about the user?

- **Plan:** lowering sets ids, dates, the default author and the
  `<<name>>` and `<<initials>>` tokens identically for every surface.
  Without the `identity` permission, which no bundled plugin asks for,
  those tokens stay literal, a plugin's marks are authored by the
  plugin, and authors read back as roles. Imported comments alone keep
  their own `/NM`, dates and author, through `imported`, which needs
  `Cap::User` (§6.8; proposal 20).
- **Rejected:** today's `sign_marks`, which fills in the user's name for
  the plugin to read back (bug 27). The synthesis's claim that plugins
  never learn identity, which the review showed false
  (`consumers/identity-guarantee-false`).
- **From:** the review; the import exception from the plan's critic, so
  that a re-import matches by `/NM` and replies thread by name.

#### D68. How does the agent address things across turns? (log F28)

- **Plan:** nothing is built for the agent, and its socket is out; the
  API leaves room (version tokens, value-validated reads,
  `revert_group`, versioned JSON). Files and CLI scripts never carry
  session ids: they address by position, title path, `/NM` and `--with`
  name (§14.2, §14.4, §20; proposal 36).
- **The log had:** session ids plus a version token, and `Session`
  preview and commit on the instance socket for `sheaf mcp`.
- **Rejected:** stable keys now: derivable from records later. The
  socket now: nothing in M10.5 uses it.
- **From:** risk-first, minimal-delivery; the review.
- **Review:** cut the socket; plan handles, per-session decision reads
  and "revert this turn" are recorded for the agent milestone.

#### D69. Is provenance written to files? (log F29)

- **Plan:** no. The actor is kept per version for the session and shown
  in History; nothing goes into the file except a mark's `/T`, which
  comes from the host (§6.7).
- **Rejected:** writing provenance to the file: it conflicts with the
  private-data goal and with sanitize.
- **From:** all five.
- **Review:** `Actor::Agent` left M10.5 as unproduced, taking the log's
  "Claude for GE" example with it.

#### D70. How does Combine work?

- **Plan:** open the first file and make one transaction (delete its
  unselected pages, insert each further file's selection, add a bookmark
  per file), save to the chosen path, and hand the same `Document` to
  the new window. The first file stays the primary, so its catalog keys
  flow through as today (§14.1–14.2; proposal 35).
- **Rejected:** an empty session plus inserts (the synthesis): the
  primary's catalog keys are lost
  (`code-truth/combine-new-empty-loses-primary`). Writing and reopening
  (today): notices and the scripts-off state can't reach the new window.
- **From:** the review.

#### D71. How does the CLI express decisions?

- **Plan:** `--force` means only "overwrite"; signature and XFA answers
  are explicit flags; exit code 3 means a decision is needed, with the
  `Ask` printed as JSON; `sheaf edit` addresses by position, title path,
  `/NM` and `--with` name; `sheaf inspect --json` is added (§14.2;
  proposal 40).
- **Rejected:** `--force` also answering signature asks (the synthesis):
  a script would have a choice made for it silently
  (`consumers/cli-force-overloaded-and-save-as-append`).
- **From:** the review.

#### D72. Is the stamp library part of the edit API?

- **Plan:** no. Library files stay outside undo, plugins and the
  private-data scan; placing a stamp interns its form (§15; proposal
  29).
- **Rejected:** putting it in the edit API (undoable, plugin-editable):
  its files are the user's own library, not document content.
- **From:** a synthesis open question.
- **Review:** settled as recommended.

### Private data and interop

#### D73. What private data does sheaf write, and where? (log F30)

- **Plan:** only `private::KEYS`: `/NM (sheaf:…)`, `/SheafAnnot` v2,
  `/SheafGenerators` (specs only), `/SheafBlock`, and `/SheafGen` and
  `/SheafOcrBlock` in named property lists. Legacy names are read only
  and stripped by `StripLegacy`; any other `Sheaf*` name fails a test
  (§15; decision 4).
- **Rejected:** JSON in BDC property lists. `/PieceInfo` for sheaf's own
  blocks: unverified, adopted only if an Acrobat test needs it (Asks).
  Model only.
- **Why:** standard keys first, O(1) recognition, and one list for
  Sanitize.
- **From:** model-definition, risk-first.
- **Review:** markers moved into named property lists, so stripping is a
  dict edit and a merged stream still classifies
  (`pdf-correctness/private-strip-impossible-in-content`); legacy names
  listed in full.

#### D74. How is OCR text tagged? (log F31)

- **Plan:** untagged invisible text, in its own block stream, wrapped in
  a `/ShOCR` marked range with a named property list (§11.4; proposal
  26).
- **Rejected:** `/Artifact`: screen readers skip it, losing a scan's
  only text. Tagged text: M14 decides tagging.
- **From:** all five.
- **Review:** the marker moved into a named property list (D73).

#### D75. Do copied widgets and pages keep structure links?

- **Plan:** copies are untagged until M14. `/StructParents` is kept only
  on copy 0 of a page whose structure tree is written; copies and pages
  from other files lose it, their annotations lose `/StructParent`, and
  a Note says so when the source was tagged (§11.3; proposal 24).
- **Rejected:** keeping `/StructParents` on a copy: it points into the
  original's parent tree. Adding OBJRs under the original's structure
  element: M14 models structure.
- **From:** a synthesis open question; every design left copies
  untagged, and the correctness lens noted none had an accessibility
  policy.
- **Review:** widened from copies to pages inserted from other structure
  trees (`future-fit/struct-parents-on-inserted-pages`); M14's structure
  keys are recorded in §20.

#### D76. What happens to PDF/A and PDF/UA claims?

- **Plan:** a conformance fact on each source, and a planner Note when a
  change breaks a claim. Conversion, font embedding and claim removal
  are M14's (§3.1, §20; proposal 39).
- **Rejected:** embedding fonts and enforcing claims now: M14's scope.
- **From:** the review (`pdf-correctness/conformance-claims`).

### Delivery

#### D77. How is the milestone delivered?

- **Plan:** incremental migration in 20 one-commit slices, about 68
  points; each deletes something into `tests/retired.rs`, updates its
  docs, extends the 7k gates and keeps a ledger of transitional code.
  Exact replay first, measured risks second (Slices).
- **The log had:** 18 slices (about 46 points), with ledger, docs and
  gates in a final slice.
- **Rejected:** a parallel core with a big-bang cutover (object-delta;
  fatal, feasibility lens). Multi-commit mega-slices (model-definition's
  slice 3).
- **From:** minimal-delivery, model-definition; the feasibility lens.
- **Review:** re-sized honestly (the synthesis tabled 50 points while
  claiming 46). Reordered so PageSpace, the content toolkit and the
  serializer precede records, and Rewrite, Append and ops follow them;
  the legacy projection lives one slice, not six. The ledger lands in
  slice 1 and the gates in slice 2.

#### D78. When does M10 resume?

- **Plan:** after all of M10.5; slice 20 rewrites m10.md's slices 6+
  (proposal 11; Greg's decision 2).
- **The log had:** after slices 1–14 at minimum, since 12–14 delete the
  derived copies M10 slice 3 taught to carry forms; 15–18 could trail.
- **Rejected:** trailing slices: Greg's sequencing builds M10's
  remaining slices on the finished API (`future-fit/m10-sequencing`).

#### D79. Which questions stayed open for Greg?

- **Plan:** three, each assuming its recommendation under Greg's
  standing instruction: proposal 8 (live page numbers), 9 (strict
  removal) and 10 (exports keep encryption). They are the first open
  item under Asks.
  Signed and certified fixtures, Acrobat-made headers and watermarks,
  and Word-tagged PDFs are requested material, not blockers.
- **The log had:** 17 open questions. The review's triage left three
  open, turned two (signature fixtures, Acrobat interop) into requested
  material, and settled twelve, all as recommended except unencodable
  text (D57). The export question lost its checkbox (D43)
  (`delivery-simplicity/open-questions-triage`).

#### D80. What did the review defer, and where to?

- **Deferred outright, with rules in §20:** to M12, how signing fits
  history, signing as model state, and "changes since signing"; to M11,
  security's two homes (`Key::Security`, kept out of serde); to M11 and
  M13, content-stack rewrite semantics; to M14, PDF/A fonts as a profile
  input; to M15, batch steps and caps; to the agent milestone, its
  revert and its entry points.
- **Deferred in part:** refusing locked fields at lowering (M10 slice
  9), gating field values in plugin renders (0.5) and for the agent, and
  the agent's read sets.
