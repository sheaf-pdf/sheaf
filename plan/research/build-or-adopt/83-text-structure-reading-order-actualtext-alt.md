# Build or adopt 83: Text structure: reading order, ActualText, alt text, text with layout

Area: M17: Accessibility (and M7's text with layout) · Part of [build or adopt](README.md)

- **Need:** reading order on untagged pages; ActualText and Alt from
  marked content and the structure tree; the page view's accessible
  text; M7's export "with layout ❓"; the Markdown export (Pending).
- **Where:** `ROADMAP.md:559-561` (the page view's tree), `:577-583`
  (structure), `:592-594` (tagging); `m11.md:174-176`; `ROADMAP.md:185`.
- **Today's plan:** the content interpreter ties content to tags
  (`ROADMAP.md:579-580`); oar-ocr's PP-DocLayoutV2 orders regions
  (`:592-594`); PDFium supplies the characters.
- **Candidates:** no Rust crate reads and edits a PDF structure tree on
  an existing file; krilla writes tags only into new files.
  pdfplumber-core's word grouping was already judged weak
  (`docs/prior-art/rust-ecosystem.md:255-260`).
- **Recommendation:** **build in-house** on the M11 interpreter
  (`marked()` already lexes BDC properties) plus oar-ocr, already
  adopted. Medium. No crate fits.
