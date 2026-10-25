# M5 research: Separators

Part of [M5: Scan & OCR](../README.md).


- **CCH Scan's target sheets** are generic, reusable barcode sheets
  (single-page, multi-page, forced classification with a class, form or
  custom bookmark name); the sheets and cover pages are **dropped** from
  the output; the cover page's second barcode names the PDF. The
  symbology isn't documented. CCH's 2011 guide: print, don't photocopy
  ("copies tend to degrade"). PDFlyer's Insert Target Sheets puts a
  sheet "above the selected page", and its Remove only takes a sheet off
  the dialog's list before OK. Scan Funnel inserts above or below, or a
  sheet before every file automatically.
- **SurePrep's** target sheets are one Letter page each with a Code 128
  barcode of the category's name (decoded); the workpaper takes the
  sheet's name, numbered on repeats.
- **Capture software** (Kofax/Tungsten, PaperStream Capture, Kodak
  Capture Pro, Epson Document Capture Pro, ABBYY, DocuWare): separate by
  barcode (type plus pattern), patch code, blank page, page count, text;
  delete the separator or keep it (an option everywhere; Kodak keeps
  patch 2 and drops 3 and T); "separate when the value changes"; file
  names from the barcode's value and tokens. Kofax prints separator
  sheets with a patch code and an optional barcode of the form name.
- **Desktop PDF editors** (Acrobat, Foxit, PDF-XChange, Power PDF) split
  by pages, size, bookmarks, blank pages or text; none splits on
  barcodes or inserts separator sheets. Evermap's AutoSplit plug-in
  splits Acrobat files by text and names them from it.
- **Open source.** Paperless-ngx splits on a separator barcode (default
  text `PATCHT`, any symbology, the page dropped unless asked), keeps ASN
  and tag barcode pages, and reads with zxing-cpp; it dropped zbar as
  unreliable on skewed and low-contrast codes. NAPS2's "Patch-T"
  separation reads only a Code 39 `PATCHT` with ZXing.NET and drops the
  sheet. Stirling-PDF splits on its own QR divider, drops it (and the
  next page in duplex mode), and decodes a page's image before drawing
  it. ecoDMS prints its "T" sheet on both sides.
- **Nobody but CCH inserts separator sheets into an existing PDF.** The
  print, rescan and re-run loop is PDFlyer's, and worth keeping.
