# Build or adopt 86: Export to Markdown, and the agent panel's Markdown

Area: Pending · Part of [build or adopt](README.md)

- **Need:** write headings, paragraphs, lists and tables in reading order
  (export); render streamed Markdown replies (the panel).
- **Where:** Pending "Export to Markdown" (`ROADMAP.md:686-705`);
  `agent.md:437`.
- **Today's plan:** export: "born-digital pages use PDFium's text plus
  heuristics … Scans can use the structure pipeline in `oar-ocr` … with
  `to_markdown()`" (`ROADMAP.md:698-703`). Panel: "our own Markdown
  renderer on `pulldown-cmark` (MIT)" (`agent.md:437`).
- **Candidates:** [pulldown-cmark](https://crates.io/crates/pulldown-cmark)
  0.13.5 (2026-10-21; MIT; 184M downloads; pure Rust);
  [comrak](https://crates.io/crates/comrak) 0.57.0 (2026-10-21;
  BSD-2-Clause), with a formatter from a CommonMark/GFM tree back to
  text.
- **Fit:** export is a serializer of sheaf's reading-order model; writing
  Markdown (escaping, GFM tables) is small. comrak's tree would be a
  second, intermediate model. pulldown-cmark can parse the output back in
  tests.
- **Recommendation:** export: **build in-house,** with pulldown-cmark in
  tests (medium). Panel: **adopt pulldown-cmark,** as planned (high).
