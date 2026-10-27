# M18 slice 1: One door for lopdf

Status: open · Part of [M18: the PDF layer](../README.md) · in sheaf

**What:** every sheaf crate reaches lopdf through one module,
`sheaf_doc::pdf`, so later slices change what it points at rather than
203 files. No behaviour changes.

- `sheaf_doc::pdf` re-exports what sheaf names of lopdf: `Object`,
  `Dictionary`, `Stream`, `ObjectId`, `StringFormat`, `Document`,
  `Error` and `Result`, the `dictionary!` macro, and the reading types
  (`LazyPdf`, `ByteSource`, `FileBytes`, `ReadLimits`, `XrefFacts`,
  `XrefKind`, `xref::XrefType`, the `encryption` items `source/` and
  `serialize/crypt.rs` use).
- sheaf-doc's own modules name it too, not `lopdf::`.
- sheaf-ops, sheaf-render, sheaf-cli, sheaf-app, sheaf-ocr and
  sheaf-plugin drop their `lopdf` dependency: each depends on sheaf-doc
  already ([sheaf.md](../sheaf.md#what-sheaf-uses-of-lopdf-2026-10-27)).
- **The inventory:** every method, function and trait sheaf calls on
  lopdf's types, with how many call sites, in this slice's notes. It's
  what [slice 3](s03-pdf-cos-editing-and-writing.md)'s owned form must
  cover and what [slice 10](s10-an-in-memory-document-of-sheaf-s-own.md)'s
  document must offer. Calls that a better API would remove are marked
  so, not kept for their own sake.

**Where:** `crates/sheaf-doc/src/lib.rs` (a `pdf` module), every
`use lopdf` in the workspace, the six crates' `Cargo.toml`.

**Checked:**
- the workspace builds, and every test passes unchanged;
- a CI step, like "sheaf-util stays byte helpers": nothing but
  sheaf-doc depends on lopdf (`cargo tree`);
- a test that no file outside `sheaf_doc::pdf` names `lopdf::`, so the
  door stays the only one.
