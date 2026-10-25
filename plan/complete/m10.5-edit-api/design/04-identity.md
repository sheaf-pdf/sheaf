# M10.5 §4: Identity

Part of [M10.5: the design](README.md).


| Id | Allocation | In the file | Stable across |
|---|---|---|---|
| `PageId(u64)` | `IdAlloc` counter | no | the session, including saves |
| `AnnotId(u64)` | marks: random in [1, 2^53). Others' annotations, links and widgets read at open: an rng seeded by the source hash, so reopening an identical file gives identical ids. Every import, duplicate or insert gets fresh ids. Duplicate `sheaf:` names get fresh ids (§3.6). | marks sheaf made: `/NM (sheaf:<id>)`; adopted and imported marks: their own `/NM`, with the id in `/SheafAnnot` v2 | everything; marks across sessions |
| `FieldId(u64)` | counter; open numbers nodes in tree order | full name | session |
| `OutlineId(u64)` | counter | no | session |
| `GenId`, `BlockId` | random in [1, 2^53) | `/SheafGenerators`, `/SheafBlock /G` and `/B` (as strings, §11.4) | sessions |
| `SourceId = BlobId` | content hash, byte-compared | no | deterministic |
| `FormInst { src, copy }` | next copy for `src` at lowering | no | session |
| `VersionId` | counter; names one immutable state | no | session |
| `GroupId` | counter | no | session |
| `DocInstance(u64)` | random per open | no | part of every version token |

- **One `IdAlloc`, in an `Arc` with atomics.** Lowering allocates every
  id and stores it in a prim's after-value, so replay never allocates.
  Counters never rewind.
- **Pinned allocation for re-lowering.** A `Relower` commit (§8.5)
  replays the ids the first lowering allocated, in allocation order per
  kind, before drawing fresh ones.
- **Ids are never derived from object numbers.**
- **Uniqueness.** A random `AnnotId` is checked against `annots` in
  O(1). If two transactions mint the same id, the second commit fails
  its `before: None` check as a conflict.
- **A version token is `{ instance, version }`.** Since a VersionId
  names one immutable state, the token alone says whether anything
  changed. Snapshots, plugin reads, autosave skips and cache keys all
  use it.
- **`ObjKey`** is the materializer's identity for what it writes
  (§11.3). Ids and object numbers meet only in the `SaveRecord`'s map.
- **No sub-page content ids in M10.5.** Sub-page identity stops at
  blocks (`/SheafBlock`, `MarkedSel`). Descriptors arrive with M11's
  interpreter.
