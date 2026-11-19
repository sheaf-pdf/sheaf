# M18 slice 13: lopdf gone

Status: done · Part of [M18: the PDF layer](../../m18-pdf-layer/README.md) · in sheaf

**What:** what's left of lopdf goes, and the vendored crate with it
([F13](../backlog-closed.md#f13-in-house-what-sheaf-uses-of-lopdf-and-drop-the-dependency)'s end).

- The last uses: `LazyPdf` and lopdf's reader where anything still
  reaches them, its writer in tests and fixtures, `decode_text_string`,
  `resources_of`.
- **Tests that use lopdf as an oracle** keep an oracle: the content
  corpus check's `Content::decode` (`content/corpus.rs`) and `forms/check.rs`
  use `pdf-content-streams` (`sheaf-doc` `[dev-dependencies]`), and
  `source::lazy_tests`' eager load compares `LazyPdf` against
  `pdf::Document` plus qpdf/PDFium.
- `vendor/lopdf/` deleted with its `SHEAF.md`; the root `Cargo.toml`'s
  `[patch]` and workspace dependency go; `packaging/licenses/LICENSE-lopdf`
  deleted.

**Where:** `vendor/lopdf/`, `Cargo.toml`, `packaging/licenses/`,
`crates/sheaf-doc/tests/pdf_door.rs`, `justfile`, `.github/workflows/ci.yml`.

**Checked:**
- `cargo tree --workspace -e all`: zero `lopdf` anywhere (neither normal
  nor dev dependencies), and zero `insecure-test-writer` in normal
  dependencies;
- `crates/*/clippy.toml` disallowed-method bans updated to
  `sheaf_doc::pdf::*`;
- every test and corpus suite; the interop oracles; the 7k gate.

## Notes

- Removed `lopdf` from workspace and fuzz manifests/lockfiles and deleted `vendor/lopdf/` and `packaging/licenses/LICENSE-lopdf`.
- Backed test-only `sheaf_doc::pdf::content::Content::decode` with `pdf-content-streams` in `sheaf-doc` `[dev-dependencies]`, updated all four crates' `clippy.toml` disallowed-method paths to `sheaf_doc::pdf::*`, and tightened `pdf_door.rs`, `just boundaries`, and CI to forbid `lopdf` everywhere and `insecure-test-writer` in normal dependencies.
