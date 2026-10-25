# M10.5 §8: History

Part of [M10.5: the design](README.md).


#### 8.1 Versions are immutable states

```rust
pub struct Step {
    pub op: Arc<ResolvedOp>,      // selectors → ids, locals → ids, BlobSel → BlobRef: what describe and merge read
    pub prims: Arc<[Prim]>,       // AUTHORITATIVE for replay; dropped once the version becomes the root
    pub effects: Effects,
    pub line: Line,               // describe(), against the state before this op (recomputed on a merged commit)
    pub cascades: Arc<[Cascade]>,
}
pub struct Version {
    pub parent: Option<VersionId>, pub children: Vec<VersionId>, pub redo_to: Option<VersionId>,
    pub label: Arc<str>, pub actor: Actor, pub at: PdfDate,
    pub steps: Vec<Step>, pub keys: Arc<KeySet>,   // union over steps: save paths, ChangeSets
    pub state: Option<DocState>, pub snapshot_bytes: usize, pub step_bytes: usize,
    pub group: Option<GroupId>, pub sealed: u16,   // > 0 while a save of this version is in flight
    pub saves: Vec<SaveNote>,                      // { path, at, strategy } of every save of this version (History panel)
    pub record: Option<Arc<SaveRecord>>,           // kept whole only while this is the saved version (§9.1)
}
```

- **A VersionId names one state, forever.** Nothing mutates a version's
  state.
- **An amend replaces the version** with a new id under the same parent.
  The parent's `children` and `redo_to` are updated, and the old id
  stops resolving.
- **The snapshot tree is otherwise unchanged:** push, `redo_to`, jump,
  timeline, branch, `MAX_VERSIONS`, and `HistoryPolicy` (200 snapshots,
  256 MB).
- **Pinned snapshots.** `keeps_snapshot` covers the root, the cursor,
  the saved version and sealed versions.

#### 8.2 Replay: exact and checked

- `ensure_state(v)` replays prims from the nearest kept snapshot, and
  `apply` asserts every before-value.
- **A `Mismatch` during replay means corruption, an internal bug.** It
  surfaces as "History is damaged at version N" in the History panel,
  marks that version unreachable, and panics in debug builds. Undo never
  quietly does nothing.
- **Replay never re-runs lowering.** What `compute` reads today without
  recording it stops mattering: `page_count`, `page_size`, the rng,
  `Document.forms` and path stems.
- **The property test covers every op:** random batches over a
  corpus-built document, with groups, branches, undo, redo, jump, random
  trims and random byte-budget pruning. Rebuilt states equal the
  snapshots under a test-only deep `Eq`, with identical ids, and one
  corrupted before-value yields `Mismatch`.

#### 8.3 describe

- `describe(op, before: &DocState) -> Line` runs inside `Tx::apply`
  against that op's pre-state, so later ops in a batch print the right
  page numbers.
- When a commit merges onto a moved cursor, the lines are recomputed
  against the merged pre-state (cheap), so "Crop page 5" is still true
  after a concurrent reorder.
- Lines are specific: "Crop pages 3–5", "Add watermark “DRAFT” behind
  all pages", "Delete pages 4–6 and 2 bookmarks". A batch of 7,000 ops
  of one kind collapses into one line.
- `VersionView`, approval cards and plugin labels read stored lines.
  describe.rs's 21-arm match goes.

#### 8.4 Groups: one undo step per job or call

A **group** makes the versions of one job or plugin call one undo step.

```rust
pub enum GroupKind { Ocr, PluginCall { plugin: Arc<str> } }      // M10 slice 11 adds Typing
impl Document {
    pub fn begin_group(&mut self, kind: GroupKind) -> GroupId;
    pub fn end_group(&mut self, g: GroupId);
    pub fn rollback_group(&mut self, g: GroupId) -> Result<Option<ChangeSet>, EditError>;
    pub fn revert_group(&mut self, g: GroupId, label: &str) -> Result<Commit, EditError>;
}
```

**The amend rule.** A commit in group `g` amends the cursor version when
all of these hold:
- the cursor version is tagged `g`;
- it has no children;
- it has no `saves`;
- it is not sealed.

The amend coalesces the prims (§6.2), recomputes the version's byte
accounting and mints a new id. If the amended version nets to identity,
it is removed and the cursor moves to its parent. Any other commit in
`g` pushes a new version tagged `g`.

**`GroupEnded`** is returned exactly when the group's latest version is
not an ancestor-or-self of the cursor, meaning the user undid it or
jumped away. Undone results are never re-applied.

**Interleaving is allowed.** If the user edits during an OCR run, OCR's
next flush pushes a new version tagged `g`, shown as "Recognize text
(continued)". The group is then several fragments. Undo steps through
them one at a time, and the History panel's "Undo all of Recognize text"
calls `revert_group`.

**`revert_group`** commits the inverse prims of all of `g`'s versions,
latest first, with before and after swapped, as one new version. It
validates by value against the cursor (§8.5). On a conflict it fails,
nothing changes, and the user is told which later edit touched the same
data.

**`rollback_group`** (a plugin trap, Err or cancel):
- **Truncate** when the group's only version is the cursor and is
  childless, unsaved and unsealed. The cursor jumps to the pre-group
  parent and the version is removed, leaving no redo entry.
- **Otherwise revert,** as `revert_group`.
- **On a conflict,** keep the group's work and notify: "Plugin X failed;
  its partial edits were kept".

**Who uses groups in M10.5:** OCR (a flush every 10 pages or 2 s) and
plugin calls (§14.3). Dialogs and drags don't. They preview with
`Tx::preview` and commit once (§8.6).

#### 8.5 Commit, preview and stale bases

A **stale base** is a transaction built on a version that is no longer
the cursor.

```rust
pub enum Base { Current, At(VersionId) }
pub enum Stale { Validate, Relower }
pub struct CommitOpts { pub label: Option<Arc<str>>, pub group: Option<GroupId>, pub base: Base, pub stale: Stale }
pub struct Commit { pub version: Option<VersionId> /* None: no-op */, pub amended: bool, pub created: Created,
                    pub change: ChangeSet, pub effects: Effects, pub lines: Arc<[Line]>,
                    pub cascades: Arc<[Cascade]>, pub notices: Vec<Notice> }
impl Document {
    pub fn tx(&self, actor: Actor) -> Tx;                          // base = cursor
    pub fn tx_at(snap: &Snapshot, cx: Arc<LowerCx>) -> Tx;         // off-thread; base = snap.version
    pub fn commit(&mut self, tx: Tx, opts: CommitOpts) -> Result<Commit, EditError>;
    pub fn undo(&mut self) -> Option<ChangeSet>; pub fn redo(&mut self) -> Option<ChangeSet>;
    pub fn jump(&mut self, v: VersionId) -> Option<ChangeSet>;
    pub fn snapshot(&self) -> Snapshot;                            // O(1): instance, version, state, store
    pub fn save_input(&mut self, v: VersionId) -> SaveInput;       // seals v; computes path_keys (§9.1)
    pub fn record_save(&mut self, rec: SaveRecord) -> ChangeSet;   // on rec.version; unseals
    pub fn abandon_save(&mut self, v: VersionId);                  // a failed or canceled save: unseals
}
```

**The commit rule is one algorithm:**

```
if tx.base == cursor:
    new state = tx.state                      // exact: VersionIds name immutable states
else:
    for (key, seen) in tx.decided: cursor[key] ≡ seen, else Conflict    // ptr_eq, then Eq
    match stale:
      Validate:
        for (key, seen) in tx.seen ∪ {first before of each written key}: cursor[key] ≡ seen, else Conflict
        new state = cursor ⊕ tx.prims        // apply re-asserts before-values; a failure is Conflict, never Mismatch
        re-run the cascade and uniqueness rules over the written keys on the new state;
        any extra prim or violation → Conflict
      Relower:
        tx' = lower(tx.ops, cursor, pinned ids)     // a lowering error is returned as is (e.g. the anchor page is gone)
        if tx.handed_out and tx'.created ≠ tx.created → Conflict
        new state = tx'.state;  prims, lines = tx'.prims, tx'.lines
coalesce prims per key; drop identities; none left → no-op (version: None)
amend if eligible (§8.4), else push;  debug: check_invariants on the touched ids
```

- **Validation by value replaces unions of path keys.**
  - It is exact under amends, because a version id never hides a changed
    state.
  - Changes that cancel out don't conflict.
  - A pruned base doesn't matter.
- **The post-merge cascade and uniqueness check closes phantoms.**
  Uniqueness covers one sheaf generator per kind and z, `/NM` per page
  and field full names, on the touched ids only. For example, a stale
  `DeletePages` while another transaction added a generator scoped to
  that page would leave a dangling scope; the check turns it into a
  Conflict.
- **`Relower` handles phantoms by construction,** because the cascades
  run on the cursor.
- **Policies:**

| Caller | Base | Stale | Decision reads |
|---|---|---|---|
| Gestures, menus, paste | Current | — | — |
| Dialogs (crop, watermark, header/footer, labels…) | the version opened on | Validate | none beyond lowering (positions were resolved to ids at open) |
| `insert_file`, blank and image inserts, comment import | the planning snapshot | Relower | none: what they planned from are source facts (immutable) |
| Blank review deletes | the planning snapshot | Relower | `Page(p)` of each page the user confirmed (what its thumbnail showed) |
| Trim White | the planning snapshot | Validate | `read_render(p, content_only)` of each measured page, which is `Page(p)`; `Geom(p)` of each page it found a crop for (slice 15's review: a page already cropped so takes no op, so no op reads it) |
| OCR flush | the run's snapshot per page | Validate | `read_render(p, content_only)` taken at render time, which is `Page(p)`: a crop or rotate during the run never conflicts |
| Plugin call (§14.3) | the call's first read | Relower | every WIT read; `created` must match |
| M10 form calculations (FormsHost) | the cascade's snapshot | Validate | the calculation's inputs |

- **Re-lowering pins ids,** so ids already handed to a plugin stay
  valid. If re-lowering would hand out different ids, the commit is a
  Conflict instead.
- **Preview ids come from the real `IdAlloc`.** A committed preview has
  the same render keys, so its tiles are reused.
- **`Snapshot` is O(1).** `SaveInput` (§9.1) is computed only on the
  save path, so snapshots carry no dirty flags.

#### 8.6 Previews

- **What `Tx::preview()` returns:** the scratch state, lines, effects,
  consequences, created ids and caps.
- **Where dialog previews run.** Previews lower on a background executor
  with a generation token. Stale results are dropped.
- **Previews follow the document.** A dialog re-lowers its preview on
  every `ChangeSet`, so a dialog left open across an undo shows the
  current state.
- **Rendering.** The realizer renders the scratch state's touched pages
  (§10.1), so the crop, header/footer and watermark dialogs show the
  page as PDFium will draw it.
- **Drags.** GPUI draws a ghost, and `ViewInputs.dragging` hides the
  originals from the realization. Mouse-up makes one commit,
  `MoveMarks { by }`, applied to the record as it is at commit, so a
  concurrent plugin or OCR change survives.

#### 8.7 The memory budget

- **Trimming snapshots** frees `snapshot_bytes` only. That is the first
  response to `HistoryPolicy` pressure.
- **Pruning versions** frees `step_bytes`. When
  `step_bytes + blob_bytes` still exceed the byte budget after trimming,
  versions are pruned in this order:
  1. off-line branches, oldest first;
  2. then the oldest versions on the line, advancing the root.
- **The root may pass the saved version.** When it does, a **detached
  saved anchor** `{ state, record, keys_to_root }` keeps what the
  planner needs, and `keys_to_root` grows as the root advances. A
  document whose anchor is gone (never saved) plans `NoSaveRecord` and
  Rewrites.
- **When a version becomes the root,** its steps drop their prims and
  keep their lines and effects, since nothing replays into the root.
- **Sealed and saved versions are never pruned.**

Under this policy, 200 re-OCRs of a 1,000-page scan (about 2.4 GB of
text layers if all were kept) stay within the budget: history gets
shorter instead of RSS growing.
