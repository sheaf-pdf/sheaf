# M18 slice 2: pdf-cos and arlington, cut from pdf-lint

Status: open · Part of [M18: the PDF layer](../README.md) · in pdf-lint's workspace, by pdf-lint's agent

**What:** the first cuts of [crates.md](../crates.md#where-pdf-lints-modules-go),
which change nothing pdf-lint does.

- **pdf-cos:** `object.rs`, `text.rs`, `version.rs`, and `bytes.rs`'s
  object parser (`value`, `name`, an indirect object's header, a
  stream's two extents, recorded and unresolved). MIT OR Apache-2.0
  (Greg relicenses what moves).
- **arlington:** `grammar.rs`, `predicate.rs`, `extension.rs`,
  `xxh64.rs` and `model/`, over pdf-cos's types (Greg's ruling,
  2026-10-27). `extension.rs` takes the catalog and the XMP metadata
  instead of a `Source`, so arlington depends on pdf-cos alone.
  Apache-2.0.
- **`text::passwords` to pdf-crypt.**
- pdf-lint keeps the rest and depends on both.

**From sheaf** ([from-sheaf.md](../from-sheaf.md#for-pdf-lint-itself)):
the `ObjectStream` check pdf-lint 0.1.0 skips, which sheaf's tests
stand in for, can land with or after arlington's cut.

**Where:** pdf-lint's workspace, `crates/pdf-lint/src/` into
`crates/pdf-cos/` and `crates/arlington/`.

**Checked:**
- pdf-lint's tests, unchanged, and its fuzz targets;
- pdf-lint's reports on sheaf's corpus, byte for byte the same before
  and after;
- each new crate's own unit tests and fuzz body, moved with its code;
- `cargo tree`: arlington doesn't reach pdf-structure's code, pdf-cos
  reaches only pdf-tokenizer.
