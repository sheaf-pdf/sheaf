# M10.5 §2: The shape

Part of [M10.5: the design](README.md).


```
 callers: gestures · dialogs · jobs (OCR, trim, import) · `sheaf edit` JSON · plugins (WIT 0.4, JSON Op)
            │  Op (public, serde + schemars, sheaf-edit crate)          Transform (export-only: ExportTx)
            ▼
 resolve    BlobSel → BlobRef (Store, CLI `--with`, plugin asset grants); caps checked per resolved target
            ▼
 Tx         lower(op) over (DocState, immutable Sources, Store, IdAlloc, HostAttrs) — pure
            │  validate → Puts → named cascade rules to a fixed point → describe line
            │  records lowering reads, write before-values, and the caller's decision reads (by value)
            ▼
 Prim::Put { key, before, after }      ← the one primitive. History stores it; replay applies it and asserts `before`.
            ▼
 DocState   order · pages(PageDef) · geoms · page_annots · annots · fields · form · bookmarks · outline_roots
            · labels · generators            (im + Arc: O(1) clone, Send + Sync; records hold BlobRef/Src handles)
     │                                   │                                        │
 classify(key, before, after)      realize(state, page group, ViewInputs)    materialize(state, Profile, SaveTarget)
 = Effects (pure)                  = emit(group) as an in-memory append      = plan → emit → (Rewrite: keep_only)
     │                               section over the source bytes              → serialize → verify
 ChangeSet · save plan ·                 │                                        │
 approvals · emit_deps             EnginePool (PDFium, one lock)            bytes + SaveRecord (never a rebase)
```

**Twelve rules:**

1. **DocState is the document's definition.** One emitter derives every
   PDF object from it, on screen and on disk. DocState holds no object
   number that can go stale. Provenance (`ObjRef`) points only into
   immutable sources.
2. **Sources are immutable, lazily read and content-addressed.**
   - A file fact is a memoized pure function of a source's bytes, so
     facts can't "arrive late".
   - Records hold liveness handles (`Src`, `BlobRef`), so a snapshot or
     a step pins every byte it names.
3. **Two layers and one primitive.** About 25 public `Op`s lower to
   `Put { key, before, after }` over 11 key kinds. Ordered collections
   are values of keys, not splice primitives.
4. **History stores resolved prims with before-values, and a `VersionId`
   names one immutable state.**
   - Replay asserts before-values and never lowers.
   - A commit onto a cursor that moved since the transaction's base
     validates, **by value**, everything it read or overwrites.
5. **Effects are a pure function of `(key, before, after)`.** Dirty
   state, removal detection, signature impact, ChangeSets, render
   invalidation and the save plan are all folds of it.
6. **One frame below the page: PageSpace,** the unrotated default user
   space. Box and rotate edits never rewrite a stored coordinate.
   Rotation is absolute. Pointers, the clipboard and plugin helpers
   convert at the edge.
7. **No id is an ObjectId, a slot or an index.**
   - Ids written to files are random below 2^53 and written as PDF
     strings.
   - Session ids are counters, and no id is ever reused.
8. **A save is an output, not an edit.**
   - The model never re-expresses a version.
   - A `SaveRecord` drives the next save, and the version being saved is
     sealed until the save finishes.
9. **One emitter** feeds four outputs: full rewrite, incremental append,
   recovery section and render overlay.
10. **The screen equals the file.** The one exception: GPUI keeps
    drawing sheaf marks until M10 slice 12.
11. **Nothing touches disk** except the user's files (and their
    same-directory temp files for atomic writes), the recovery sidecar
    (encrypted when the file is) and the append journal. The other
    exceptions hold no document content, apart from an attachment the
    user opens (§10.6).
12. **Fail, never repair, and fail closed on removal.**
    - Unknown references and unreadable content are typed errors.
    - An object-level check vetoes any Append that would leave saved
      text readable.
    - The one time-boxed exception is the `'?'` glyph for unencodable
      text, until M10 slice 7 (§13).

**The registry rule.** Every cross-cutting function starts with an
exhaustive destructure of `DocState` (no `..`) or an exhaustive match on
`Key`. Those functions are `classify`, every cascade trigger,
`emit_roots`, `emit_deps`, `ChangeSet::from_keys`, `HeapSize`,
`check_invariants` and `render_key`. Adding a key kind then fails to
compile until each of them handles it.

**Nothing ships unproduced.** No key, enum variant, field or public type
ships without an M10.5 producer and consumer. §19 lists each public type
with what it replaces and what produces it. Later milestones add
variants additively; `#[non_exhaustive]` plus the registry rule make
each addition compile-checked.
