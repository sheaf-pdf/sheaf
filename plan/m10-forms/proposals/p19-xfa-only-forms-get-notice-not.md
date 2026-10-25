# M10 proposal 19: XFA-only forms get a notice, not support

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**XFA-only forms get a notice, not support.**
- On open, a form is classed as AcroForm, hybrid (an AcroForm and
  `/XFA`), or XFA-only (`/NeedsRendering`, or `/XFA` with no
  widgets). PDFium's `form_type()` tells them apart too.
- XFA-only: a bar above the page, "This is an XFA form. sheaf can't
  show or fill it; Adobe Acrobat or Reader can.", and a row in
  Properties. sheaf shows the file's fallback pages, as PDFium does
  today, and page tools still work.
- Hybrid: filled through its AcroForm, by proposal 6.
- None of the 40 corpus forms is XFA-only, so the test fixture is
  built in code, and a real sample is in the Asks.
