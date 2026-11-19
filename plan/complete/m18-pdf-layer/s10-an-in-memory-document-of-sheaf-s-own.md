# M18 slice 10: An in-memory document of sheaf's own

Status: done · Part of [M18: the PDF layer](../../m18-pdf-layer/README.md) · in sheaf

**What:** lopdf's eager `Document` leaves sheaf's production code
([sheaf.md](../../m18-pdf-layer/sheaf.md#what-sheaf-uses-of-lopdf-2026-10-27): 39 files),
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
  [slice 11](../../m18-pdf-layer/slices/s11-pdf-objects-types-through-sheaf.md) changes them with
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

## Notes

Done 2026-11-18:

- Added `crates/sheaf-doc/src/pdf/document.rs` (`sheaf_doc::pdf::Document`),
  holding `version`, `trailer`, `objects`, `max_id`, `xref_kind`, and
  `xref_start`, loaded whole in one pass through `pdf-structure`
  (`LazyPdf` / `Reading`) and written through `crate::serialize` (`Section`,
  `object::indirect`, `objstm::container`).
- Dropped `lopdf::Document`, `IncrementalDocument`, `ObjectStream`,
  `SaveOptions`, `LoadOptions`, `decode_text_string`, and `xref::XrefType`
  from `sheaf_doc::pdf`'s re-exports; `forms::fixture::append_revision` now
  writes incremental updates through `crate::serialize::append::section`.
- Collapsed the 81 `lopdf` eager-load quirk rows out of
  `source::lazy_tests::EXPLAINED` now that eager and lazy loads both run
  through `pdf-structure`.
