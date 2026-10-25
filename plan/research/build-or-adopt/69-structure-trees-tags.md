# Build or adopt 69: Structure trees and tags (M14 and M17)

Area: M14: Document processing · Part of [build or adopt](README.md)

- **Need:** model `/StructTreeRoot`, `/ParentTree` (a number tree),
  `StructElem` kids, MCIDs and OBJRs; emit them from the model; tag what
  sheaf writes or mark it as an artifact; keep a tagged file tagged
  across edits.
- **Where:** M10.5 §20 "Structure" (`m10.5-edit-api.md:3671-3673`) and
  M10.5's D75 (`m10.5-decisions.md:1769-1783`); `ROADMAP.md:576-602` (M17); M10's
  copies "untagged until M14" (`m10.md:1893-1901`).
- **Today's plan:** "`Key::StructElem`, `/ParentTree` emitted from the
  model, and `PlaceTag::Struct(elem)` for flatten"
  (`m10.5-edit-api.md:3671-3673`).
- **Candidates:** no crate edits an existing structure tree. krilla tags
  new documents only. pdf-writer has typed writers (`structure.rs`).
  pdf_oxide has a structure *reader* (`src/structure/parser.rs`,
  `traversal.rs`). pdf.js reads structure trees for accessibility
  (`src/core/struct_tree.js`, Apache-2.0), and PDFBox has
  `PDStructureTreeRoot` (Apache-2.0); both are reading models.
- **Fit:** the tree is part of sheaf's model and history (keys, Puts,
  removal), so no outside type can own it.
- **Recommendation:** **build in-house.** Use krilla's `PDF_UA1.md` rules
  ("krilla never includes artifacts in the structure tree", role maps for
  non-standard types) and pdf-writer's key names as references, and
  veraPDF `ua1`/`ua2` plus Arlington in tests. High.
