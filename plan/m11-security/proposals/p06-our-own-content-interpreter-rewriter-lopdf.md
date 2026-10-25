# M11 proposal 6: **Our own content interpreter and rewriter, on lopdf. PDFium only

Part of [M11: Security & redaction](../README.md) · [all proposals](README.md)

**Our own content interpreter and rewriter, on lopdf. PDFium only
finds and checks.**
- PDFium has no redaction API, and its page-object editing can't
  remove single glyphs.
- Its `FPDFPage_GenerateContent` isn't safe to rely on:
  - it drops shadings and inline images;
  - it writes CMYK, Separation and Pattern colors as black;
  - it drops `Tc`, `Tw`, `Tz`, `TL` and `Ts`;
  - it leaves an unbalanced `q BT` on Type3 text;
  - it rewrites a shared Form XObject in place, changing every page
    that uses it.
- So the interpreter lives in `sheaf-doc` (the roadmap's shared
  infrastructure for M13 and M14). PDFium renders and extracts text
  from the output to check it.
- It builds on M10.5's `sheaf-doc/src/content/` (its lexer, splicer,
  `Balance` and `TextBag`; M10.5 §13), not on lopdf's `Content`,
  which M10.5 retired (its proposal 17).
