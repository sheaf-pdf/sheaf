# M5 proposal 13: sheaf's separator sheet

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**sheaf's separator sheet.** One page, the document's page size
(Letter or A4 by default), black on white, mostly empty so it's
easy to see in a stack:
- **Top:** "SEPARATOR SHEET" in small capitals, and what it does:
  "Starts a new file", "Starts a bookmark", "Starts a bookmark under
  the one before".
- **Middle:** the title, large and bold (Helvetica-Bold, shrunk to
  fit three lines; characters it can't draw show as `?` until M10
  slice 7's fallback font, while the code keeps the title exactly).
- **Two QR codes** with the same text, top right and bottom left,
  modules of 0.75 mm (4.4 pixels at 150 dpi; the bench read modules
  half that size in every 1-bit 150 and 200 dpi case), error
  correction level M, at least 10 mm of white around each. A typical
  title makes a code about 30 mm square; the longest (120 bytes)
  about 40 mm. Either code survives a staple, a hole punch, a fold or
  a pen mark, and QR reads at any rotation, so a sheet fed upside down
  or sideways works.
- **Optionally, patch T bars** along the edges, in Kodak's layout
  (off by default): a sheaf sheet then also separates in PaperStream,
  Kofax or a scanner's own patch reader. The QR code still says what
  sheaf does with it.
- **Bottom:** "Put this sheet face up in front of the pages it names.
  sheaf finds it when the stack is scanned. Print it at 100%; don't
  photocopy it." and the code's text in small type, for a person.
  (CCH and Alliance give the same printing advice: copies degrade.)
- **The code's text, version 1:** `SHEAFSEP1|a=split|l=1|p=drop|t=W-2
  Acme Corp`. Fields are `|`-separated `key=value`; `t` (the title,
  UTF-8, up to 120 bytes) is always last and may contain `|`;
  unknown keys are ignored, so later versions can add fields (client,
  year) without breaking older readers; a newer major version
  (`SHEAFSEP2`) is reported in the review, not guessed at. `a` is
  `bookmark`, `split` or `both`; `l` is `1` (top) or `2` (under the
  last top one, or top while there's none); `p` is `drop` or `keep`.
- **Made in memory** (`memory::separator`, `MemoryKind::Separator`)
  from the sheet's spec, deterministically: the same spec makes the
  same bytes, as blank pages do, so an insert lowered again or
  replayed finds its pages. Drawn with the content writer: text in
  the standard 14, each QR module a filled rectangle (crisp at any
  printer resolution, a few kilobytes a page). The modules come from
  the reader library's own QR encoder (rxing's `BitMatrix`), runs
  merged into rectangles.
- The symbology is ask 8: QR is recommended (TWAIN's barcode types
  have QR and no Data Matrix, should a driver's reader ever be used).
