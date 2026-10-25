# M5 proposal 14: **Patch codes and other software's barcodes are read; sheaf prints

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**Patch codes and other software's barcodes are read; sheaf prints
only its own sheet.**
- **Kodak patch codes** are four bars along a sheet's edge, wide
  (0.20 in) and narrow (0.08 in), set in from the edge (Kodak's
  A-61599 says at least 0.25 in; Alliance's printed sheet, citing
  Kodak's A-61580, 0.5 in). They carry no data, a type only:
  1 is WWNN, 2 WNNW, 3 WNWN, 4 NWWN, 6 NNWW, T NWNW, read from the
  edge inward, which also tells 3 from T and 1 from 6 at any
  rotation. sheaf reads them itself (no barcode library does;
  proposal 15). Defaults by Kodak Capture Pro's convention, each
  changeable in the rule: **3** (a batch) starts a top-level section,
  a new file when splitting, and is dropped; **T** starts a section
  under it and is dropped; **2** does the same as T and is kept;
  **1, 4 and 6** (image controls) are listed in the review and do
  nothing. With no 3 in a batch, T and 2 sections are top-level.
- **The Code 39 text Kodak prints on its patch sheets** (`PATCH1`…
  `PATCHT`) is read as the same type, for a sheet whose bars a scan
  lost; the bars come first (in the bench, rxing missed Kodak's own
  `PATCHT` at 200 dpi in black and white, and the bars were found).
- **Code 39 `PATCHT` alone**, the only thing NAPS2 and Paperless-ngx
  look for, is read as a patch T too, so a firm with their sheets
  needs no new ones. In the dialog it's named apart from real patch
  bars.
- **Other software's barcode sheets** by symbology and pattern, with
  the code's text as the title: SurePrep's target sheets are Code 128
  with the category's name ("Schedule A", "Rent and Royalty"), so
  `code128:^(Schedule [ACEF]|Business Income|Rent and Royalty|Farm
  Income|State|Taxpayer Information)$` bookmarks a SurePrep-sheeted
  batch by category.
- **sheaf prints none of them:** patch sheets come from scanner makers
  and Alliance as printable PDFs, and sheaf's own sheet does more.
