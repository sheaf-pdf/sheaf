# M10 proposal 16: **Detection proposes fields for review; it never adds them on its

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**Detection proposes fields for review; it never adds them on its
own.**
- **Born-digital pages:** runs of underscores and box glyphs (☐)
  from `Renderer::page_chars` (sheaf-render lib.rs:133), and stroked
  lines and boxes from PDFium's path objects. Reading paths is new
  work, shared with the table finder and M9 decision 9's snapping.
- **Scans:** render, binarize (Sauvola, sheaf-image clean.rs:675),
  despeckle (:815), then find lines and boxes (new, in sheaf-image).
  OCR words give labels and rule out underlined text. A page with no
  text is offered OCR first, as Acrobat does.
- **Types:** an underline or box is a text field; a small square or
  circle a check box; a run of equal cells a comb with `MaxLen`; a
  "Signature" label a signature field; Date, SSN, EIN, ZIP, Phone and
  $ labels get the matching AF format.
- **Review:** proposals show as dashed frames, named from the nearest
  label. Accept or reject one at a time or all; accepting is one undo
  step.
- **Measured on the corpus:** flatten a corpus form with its fields
  empty, detect, and compare with the original widgets. Real forms
  give a ground truth for free.
