# Build or adopt 99: Fuzzing and property tests

Area: Infrastructure that's built (confirmed) · Part of [build or adopt](README.md)

- **Today:** `fuzz/` has seven libFuzzer targets (`lazy_reader`, `lexer`,
  `op_json`, `page_tree`, `sheaf_annot`, `sheaf_generators`, `splice`) on
  `libfuzzer-sys = "0.4"`; `proptest = "1"` in sheaf-doc, at 1.12.0
  (2026-10-10); cargo-fuzz 0.13.2 (2026-06-09) is the latest.
- **License:** libfuzzer-sys is "(MIT OR Apache-2.0) AND NCSA"; NCSA is
  LLVM's old permissive license, and it's test-only.
- **Recommendation:** **adopt** (keep). For F13, every in-housed reader
  piece gets its fuzz body before it lands, as `source::fuzz` does now.
  High.
