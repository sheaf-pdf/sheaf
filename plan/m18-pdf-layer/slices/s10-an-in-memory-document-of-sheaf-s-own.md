# M18 slice 10: An in-memory document of sheaf's own

Status: open · Part of [M18: the PDF layer](../README.md) · in sheaf

**What:** lopdf's eager `Document` leaves sheaf's production code
([sheaf.md](../sheaf.md#what-sheaf-uses-of-lopdf-2026-10-27): 39 files),
for a document type behind the door with only what sheaf calls on it
([slice 1](s01-one-door-for-lopdf.md)'s inventory): objects by id, the
trailer, new ids, adding, pruning what nothing reaches, walking.

- **Loaded whole** through pdf-structure ([slice 8](s08-sheaf-reads-through-pdf-structure.md)),
  for the Rewrite graph (`materialize/graph.rs`, `copy.rs`, `kept.rs`,
  `number.rs`, `verify.rs`) and `recovery.rs`. If
  [F15](../../backlog/F15.md) has landed, the Rewrite graph reads
  lazily already and only the rest moves.
- **Built in memory** for the PDFs sheaf makes: `annot::tape_pdf`,
  `serialize/built.rs`, the stamp library, the fixtures, sheaf-app's
  OCR and scan runs.
- Its objects are the door's types, so
  [slice 11](s11-pdf-cos-s-types-through-sheaf.md) changes them with
  everything else. Whether it then becomes pdf-structure's `Document`
  (pdf-lint's `source::Document` is an owned document readers fill and
  tests build) is decided there.

**Where:** `sheaf_doc::pdf`, `materialize/`, `serialize/built.rs`,
`annot.rs`, `recovery.rs`, the `fixture.rs` modules, sheaf-app's
`ocr_run.rs` and `scan_run.rs`.

**Checked:**
- `materialize_corpus`: counts and output bytes as before;
- every test and corpus suite, unchanged;
- the 7k gate's memory and time within their flags (F15's gate is its
  own);
- no production file outside the door names lopdf's `Document`.
