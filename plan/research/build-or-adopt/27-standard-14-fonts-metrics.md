# Build or adopt 27: The standard 14 fonts' metrics

Area: M11: Security and redaction · Part of [build or adopt](README.md)

- **Need:** widths, glyph boxes, ascender and descender, and built-in
  encodings for the 14 base fonts.
- **Where:** M10.5 (done); M11 slice 3; M10 slice 6.
- **Today:** built. `crates/sheaf-doc/src/content/font/afm.rs` parses
  Adobe's 14 AFMs (`assets/afm`, `assets/LICENSE-afm`, also in
  `packaging/licenses`); `std14.rs` has exact encoders; the interpreter
  branch adds boxes, `Ascender`/`Descender` and `FontBBox`.
  `m10.md:590-595` is stale ("the full AFM tables, and the other 12
  faces, are new"; `stamps.rs:207-254` is gone).
- **Candidates:** none worth taking; each has one maintainer and few
  downloads, so none beats sheaf's ~100 lines plus Adobe's files:
  [`afm`](https://crates.io/crates/afm) 0.1.2 (2018, 6.6k downloads),
  [`pdf-core-14-font-afms`](https://crates.io/crates/pdf-core-14-font-afms)
  0.1.1 (2024, 3.6k),
  [`pdf-base14-metrics`](https://crates.io/crates/pdf-base14-metrics) and
  [`adobe-font-metrics`](https://crates.io/crates/adobe-font-metrics)
  0.0.2 (2026-09, under 3k each).
- **Recommendation:** **build in-house (keep).** High. PDFium draws
  unembedded standard 14 fonts with its own Foxit substitutes, so its
  boxes can differ from AFM boxes; slice 3's tolerance absorbs that.
