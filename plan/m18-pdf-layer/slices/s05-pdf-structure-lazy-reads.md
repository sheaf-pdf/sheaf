# M18 slice 5: pdf-structure, lazy reads

Status: open · Part of [M18: the PDF layer](../README.md) · in pdf-lint's workspace, by pdf-lint's agent

**What:** one reader, two strategies (the notes): pdf-lint's read whole,
and sheaf's read on demand, which `LazyPdf` does today
([crates.md](../crates.md#where-lazypdf-goes-pdf-structure-not-pdf-semantics)).
`LazyPdf` is the model and the oracle, not code to copy.

- **A byte source read by position:** in memory, or a file read in
  windows (`pread`), as `ByteSource` and `FileBytes` are.
- **One limits value** for the reader's caps, as `ReadLimits` is:
  nesting, sections, a stream's decoded size, the budget in all. pdf-lint's
  `NESTING`, `SECTIONS`, `STREAM` and `BUDGET` become its defaults.
- **On open:** the header, the cross-reference sections and the trailer,
  and the security handler opened with the passwords given.
- **On demand:** each object when it's first asked for, kept, or read
  without keeping (as `object_unkept`, for a flatten that reads
  thousands once); each object stream decoded once. Shared across
  threads, as `LazyPdf` is.
- **What it records at the first parse:** each object's span and both
  stream extents, so nobody parses twice; where each object came from
  (the file, a container's decoded data, recovered).
- **The facts as written:** the cross-reference kinds and hybrids
  (`XrefFacts`), whether the file read soundly (`base_sound`), the
  linearization dictionary's `/L` (B3's check).
- **Read whole** is every object read through the same code: pdf-lint
  reads that way.

**Where:** `crates/pdf-structure/`.

**Checked:**
- pdf-lint's tests and corpus reports, unchanged, through the new code;
- whole and lazy agree, object for object, on sheaf's corpus;
- a fuzz body that opens and reads lazily, under the caps;
- open and per-object times beside `LazyPdf`'s on sheaf's heavy 7k
  file, a bench here; [slice 8](s08-sheaf-reads-through-pdf-structure.md)
  holds the 7k gate.
