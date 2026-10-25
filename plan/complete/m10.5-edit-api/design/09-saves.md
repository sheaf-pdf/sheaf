# M10.5 §9: Saves

Part of [M10.5: the design](README.md).


#### 9.1 Saves are outputs

```rust
pub struct SaveRecord {
    pub version: VersionId, pub path: Arc<Path>, pub at: PdfDate, pub strategy: StrategyKind,
    pub stamp: FileStamp,            // len, mtime, xxh3 of the last 64 KiB, xxh3 of the 4 KiB at the last startxref target
    pub content_hash: ContentHash,   // xxh3-128 of the whole file: computed while writing, lazily for an opened
                                     // file, and extended by each Append (the record keeps the streaming state)
    pub xref: XrefFacts,             // kind (table | stream), start, size, /ID, header offset
    pub crypt: Option<Arc<Crypt>>,
    pub map: Arc<ObjMap>,            // ObjKey → (num, gen) for every object sheaf WROTE into this file
    pub present: Arc<BitSet>,        // primary-source object numbers live in this file (keep_only pruned the rest)
    pub roots: Arc<RootValues>,      // the written values of UNOWNED keys of Catalog, AcroForm, Outlines root, Names roots
    pub tree: Arc<TreeShape>,        // the file's /Pages nodes, kids, and each node's size at open
    pub drawn: Arc<HashMap<(PageId, GenId), (u128 /* layout */, u128 /* recipe */)>>,
    pub transforms: Arc<[SaveTransform]>,       // what save policy already did to this file
    pub pending_tail: Option<PendingTail>,      // a restored document's recovery sections not yet on disk (§11.10)
}
pub struct PendingTail { pub bytes: Arc<[u8]>, pub removes: bool /* the sections removed or replaced saved text */ }
pub struct SaveInput { pub target: DocState, pub saved: Option<SavedSide>, pub path_keys: Arc<KeySet> }
pub struct SavedSide { pub state: DocState, pub record: Arc<SaveRecord> }
```

A **save record** says what sheaf knows about the file it last wrote:
where every object sheaf wrote sits, which original objects are still
there, and what policy already did to it. It drives the next save.

**On open,** the opened file's record is synthesized:
- an identity map;
- `present` set to every object number in use;
- `roots` from the source's catalog-level objects;
- `tree` from the source;
- `drawn` from the `/SheafBlock` markers;
- `transforms` empty.

The first incremental save is therefore not a special case.

**A save:**
1. `save_input(V)` seals V.
2. Plan in the background (§9.2), and ask any `Ask`s.
3. `materialize` in the background.
4. `record_save` sets `Document.saved = V`, sets V's `record` (reducing
   the previous saved version's record to its `SaveNote`), appends a
   `SaveNote`, unseals V, and handles the recovery sidecar (§11.10).

No version, state or source changes. Dirty is `saved != cursor`, revert
is a jump, and undo across saves is exact.

**Edits during a save are safe.**
- V is sealed, so an OCR flush or a later typing commit can't amend it.
  They push a child version, which stays dirty and goes into the next
  save.
- `rollback_group`, pruning and `remove_subtree` never remove a sealed
  or saved version.
- A second save waits for the first, because its base is the first
  save's file.

**An Unchanged plan** (the path's effects are empty, for example after
an undo back to the saved state by a different route) writes nothing. It
attaches the current record to the cursor, so `saved = cursor` and the
document is clean.

**The next Append** writes the emission of the closure of
`path_keys(saved, target)` under `emit_deps`:
- numbered through `map`, so a changed mark is rewritten under the
  **same** number;
- new objects numbered from `xref.size`;
- in the base's xref kind;
- with `/Prev` at the real `start`.

**`present`: undo across a pruning save.** Delete pages, Rewrite
(`keep_only` prunes their objects), undo, save. The emission references
base objects absent from the saved file, so the planner sees
`BaseObjectsMissing` and the save is a Rewrite. The check costs one bit
test per base reference in the emitted closure.

**Root ownership: one static table per root object,** generated from the
registry.
- **Owned keys are always emitted as a function of the target state,
  never taken from `roots`:**
  - the Catalog's `/Pages`, `/Outlines`, `/PageLabels`, `/AcroForm`,
    `/SheafGenerators` and `/Version` (§11.7);
  - the AcroForm's `/Fields`, `/CO`, `/DA`, `/Q`, `/DR`,
    `/NeedAppearances` and `/SigFlags`;
  - the Outlines root's `/First`, `/Last` and `/Count`.
- **Unowned keys come from `roots`, the values actually written.** Keys
  `keep_only` removed (an `/OpenAction` to a deleted page, a pruned
  `/Names` entry) and keys a save transform dropped (`/XFA`,
  `/Perms /UR3`) stay gone.
- **Ownership is static.** A key never becomes owned only while patched,
  so set → Append → undo → Append → reopen equals the original for every
  owned key (tested per key).
- **On Rewrite,** unowned keys come from the source and are pruned by
  `keep_only` against the target page set.

**Save As is a Save.** It goes through plan, ask, materialize and
`record_save`, and its record becomes the saved file.
- **Unsigned documents** are Rewritten (`OtherTarget`). With stable
  numbering, `Src(primary, n)` objects sit at `n` in the new file.
- **Documents with any signature** use `Append { onto: CopyOf }`
  whenever the other Append conditions hold (proposal 32). The saved
  file's bytes are reflinked, or stream-copied and verified against
  `content_hash`, to the new path, and the section is appended there.
  The new record is the old one with the new path and stamp. Signatures
  survive Save As, as they do in Acrobat.

**Older records** are reduced to `{ path, at, strategy }` for the
History panel. Only the saved version's record is kept whole.

**The cost.** Sources referenced by the current state stay alive for the
session. Opening a huge file, deleting most of it and continuing keeps
its bytes until the document is closed. (M14's Save Optimized writes a
copy and reopens it as a new session; see §20.)

#### 9.2 Strategy

```rust
pub struct SavePlan { pub strategy: Strategy, pub asks: Vec<Ask>, pub transforms: Vec<SaveTransform>, pub notes: Vec<Note> }
pub enum Strategy { Unchanged, Append { onto: Onto }, Rewrite { why: Vec<Why> } }
pub enum Onto { Saved, CopyOf }       // CopyOf: Save As of a signed document — copy the saved file, then append
pub enum Why { OtherTarget, NoSaveRecord, BaseUnsound, Removes(Vec<Removed>), ReplacesText(Vec<Removed>),
               BaseObjectsMissing, TransformNeedsRewrite(SaveTransformKind), RemovalCheck { key: ObjKey },
               RecoveredRemovals /* §11.10 */, LegacyHeaderFooter /* transitional until slice 17 */ }
pub enum Ask {
    FileChanged { path: Arc<Path> },                                   // choices: Save As…, Overwrite, Cancel
    Signatures { impact: Vec<SigImpact>, append_anyway: Option<AppendAnyway>, reason: Option<Why> },
    XfaHoldsValues { fields: usize },                                  // dropping XFA that holds values
}
pub struct AppendAnyway { pub stays_readable: Vec<Removed> }          // what the earlier revision keeps
```

**Append holds only when all of these are true.** Otherwise the save is
a **Rewrite**, with reasons.
- **The target** is the saved file, or (`Onto::CopyOf`) a new path being
  written by Save As of a document that has a signature.
- **The file.** The target's `FileStamp` (§9.1) still matches. If it
  doesn't, nothing is written and the plan asks `FileChanged` (proposal
  33).
- **The base** is sound.
- **The path's effects** include neither `REMOVES` nor `REPLACES_TEXT`.
  `LIVE_REDRAW` is fine.
- **Base objects.** Every base object the emission closure references is
  present in the saved file (`SaveRecord.present`).
- **Transforms.** No pending save transform needs a rewrite (§11.8).
- **Encryption** hasn't changed (M11's first producer).
- **A save record exists.** An untitled or recovered-without-record
  document has none: `NoSaveRecord`.

**The strategy is decided before a byte is written.** Two re-checks
still happen at write time:
- The file stamp is re-checked immediately before the rename or the
  in-place append. A mismatch aborts to `FileChanged`.
- The removal check (§11.9) runs on the finished section. A veto aborts:
  - on a document with any known signature, to
    `Ask::Signatures { reason: RemovalCheck }`, which offers Rewrite or
    Cancel;
  - otherwise, the save becomes a Rewrite.

**Strict removal** (proposal 9). Deleted and replaced text never survive
in the bytes. A regenerated page number is the one exemption. Geometry
and style changes append.

**"Append anyway"** is offered on a document with signatures whenever
every Rewrite reason is one of:
- `Removes`;
- `ReplacesText`;
- `TransformNeedsRewrite(DropXfa | DropUr3 | FixSigFlags)`;
- `RemovalCheck`;
- `RecoveredRemovals`.

In that mode the emitter applies the save transforms **inside the
appended section**, as M10 slice 5's `Mode::Append` does today, and
`stays_readable` names what the earlier revision keeps: removed pages
and comments, replaced values, and the XFA datasets. It is never offered
for `BaseUnsound`, `BaseObjectsMissing`, `FileChanged` or
`NoSaveRecord`.

#### 9.3 Signatures

`SigImpact` is computed per signature, from `Source::signatures()` of
the saved file's base, against the path's `SigClass` set. M12 adds
signatures recorded in `SaveRecord`s.

| Pending | P=1 | P=2 (allows FILL) | P=3 (allows FILL \| ANNOT) | Approval signature, no MDP |
|---|---|---|---|---|
| Rewrite | invalid: ask | invalid: ask | invalid: ask | invalid: ask |
| Append ⊆ allowed, unlocked fields | invalid: ask | allowed | allowed | allowed, noted |
| Append ⊄ allowed | invalid: ask | invalid: ask | invalid: ask | bytes valid; ask (viewers flag it) |

- **FieldMDP** evaluates `/Action /All`, `/Include` and `/Exclude` with
  `/Fields`, and PDF 2.0's `/Lock /P`, against the `Field` keys touched.
- **A save transform in an Append that edits the Catalog or AcroForm**
  adds OTHER to the Append's class: `DropXfa`, `DropUr3`, `FixSigFlags`,
  `FillAppearances`, and a Catalog `/Version` bump (§11.7). A certified
  file therefore asks.
- **The mapping** follows ISO 32000-1 Table 254 and ISO 32000-2 Table 257.
  It stays conservative until Acrobat cross-checks it (Asks).
- **Preview.** `Tx::preview` reports DocMDP consequences per op ("this
  breaks 1 signature when saved"), so an approval card or dialog says so
  before the save. Refusing locked fields at lowering arrives with the
  first field-editing ops (M10 slice 9).
