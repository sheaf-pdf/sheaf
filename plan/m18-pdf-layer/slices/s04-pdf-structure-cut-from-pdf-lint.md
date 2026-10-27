# M18 slice 4: pdf-structure, cut from pdf-lint

Status: open · Part of [M18: the PDF layer](../README.md) · in pdf-lint's workspace, by pdf-lint's agent

**What:** the reader leaves pdf-lint
([crates.md](../crates.md#where-pdf-lints-modules-go)), unchanged in
what it reads.

- **pdf-structure:** the rest of `bytes.rs` (the header,
  cross-reference tables and streams along `/Prev`, recovery by
  scanning, object streams, decryption through pdf-crypt, decoding
  through pdf-filters, the budgets), `filter.rs` and `source.rs` (the
  `Source` trait, `Document`, `Trailer`, `Warning`, `Update`,
  `Encryption`).
- It depends on pdf-objects, pdf-filters and pdf-crypt directly; qpdf stays
  a test oracle through the crate-private `Decode`.
- What's left in pdf-lint is the checker: `check.rs`, `allowance.rs`,
  `main.rs`.

**From sheaf** ([from-sheaf.md](../from-sheaf.md#the-rest-of-the-writer-slices-3-6-and-7)):
`serialize/crypt.rs`'s list of what's encrypted and what isn't, to set
beside the reader's.

**Where:** `crates/pdf-lint/src/` into `crates/pdf-structure/`.

**Checked:**
- pdf-lint's tests and fuzz targets, unchanged;
- pdf-lint's reports on sheaf's corpus, byte for byte the same;
- the reader's own tests, its qpdf comparisons among them, moved with
  it.
