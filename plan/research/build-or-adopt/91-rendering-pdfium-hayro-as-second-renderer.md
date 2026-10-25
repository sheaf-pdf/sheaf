# Build or adopt 91: Rendering: PDFium, and hayro as a second renderer

Area: Pending · Part of [build or adopt](README.md)

The images section's hayro detail, with core's PDFium facts and the
fonts section's notes on hayro's font stack.
- **Need:** a second `Renderer` behind the trait, or a test oracle.
- **Where:** `docs/prior-art/rust-ecosystem.md:18` ("Candidate second
  `Renderer` backend once it matures") and `:234`;
  `m10.5-edit-api.md:6153` ("hayro as an engine: not planned; an oracle
  only"); F17 (hayro-interpret as a second corpus oracle, added by the
  content interpreter PR).
- **PDFium:** through pdfium-render 0.9.4, the latest (2026-09-06).
  sheaf pins the binary at chromium/7881 (`docs/document-model.md:2574`);
  bblanchon's latest build is chromium/8111 (2026-10-19). Nothing to
  change beyond a routine bump.
- **hayro now:** [`hayro`](https://crates.io/crates/hayro) 0.8.0
  (2026-10-04; 9 releases since 2025-07; MIT OR Apache-2.0, GitHub reads
  Apache-2.0; LaurenzV; 814 stars).
  - Very active: about fifteen commits on 2026-10-22 and 23 alone, with
    `hayro-fuzz` and `hayro-bench` in the repository.
  - [README](https://github.com/LaurenzV/hayro): still "experimental,
    work-in-progress"; no knockout groups and no non-embedded CID fonts;
    "performance … has not been a focus at all so far but will become a
    priority in the near future"; 1,400+ test PDFs; MSRV 1.92.
  - Users: Typst's `typst-svg` and `typst-render` (^0.7.1) and
    `typst-library` (hayro-syntax); a dev-dependency of oar-ocr 0.10.1.
  - `hayro` 0.8 renders with `vello_cpu` and `pic-scale`;
    `hayro-interpret` uses `moxcms`, `skrifa` (since 2026-04, dropping
    hayro-font) and `hayro-cmap`.
  - The ecosystem notes' "Pure, `#![forbid(unsafe)]`" (`:234`) is partly
    stale: the codecs' default `simd` feature uses `fearless_simd`, and
    `hayro` itself depends on `fearless_simd` and `pic-scale` (SIMD).
- **Recommendation:** keep PDFium. hayro: **oracle only,** as Greg leans.
  Adopt its codecs now (entry 33) and its renderer not yet; re-check at
  M14's kickoff. Medium.
