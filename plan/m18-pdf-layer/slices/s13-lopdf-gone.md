# M18 slice 13: lopdf gone

Status: open · Part of [M18: the PDF layer](../README.md) · in sheaf

**What:** what's left of lopdf goes, and the vendored crate with it
([F13](../../backlog/F13.md)'s end).

- The last uses: `LazyPdf` and lopdf's reader where anything still
  reaches them, its writer in tests and fixtures, `decode_text_string`,
  `resources_of`.
- **Tests that use lopdf as an oracle** keep an oracle: the content
  corpus check's `Content::decode` (`content/corpus.rs`), the forms
  check's, and `source::lazy_tests`' eager load. Either crates.io's
  lopdf stays as a dev-dependency there, as qpdf and pdf.js are
  oracles, or the comparison moves to qpdf or PDFium. No test loses its
  oracle.
- `vendor/lopdf/` deleted with its `SHEAF.md`; the root `Cargo.toml`'s
  `[patch]` and workspace dependency go; `packaging/licenses/` keeps a
  lopdf licence only if a shipped crate still carries its code.
- Retired names in `retired.rs`, as for the Arlington checker.

**Where:** `vendor/lopdf/`, `Cargo.toml`, `packaging/licenses/`,
`crates/sheaf-doc/tests/retired.rs`, and wherever the last uses are.

**Checked:**
- `cargo tree`: lopdf only as a dev-dependency, if at all; slice 1's CI
  step becomes "no lopdf in a shipped build";
- clippy `-D warnings` and the lint bans now cover the whole PDF layer:
  what they find is fixed or logged in the backlog;
- every test and corpus suite; the interop oracles; the 7k gate.
