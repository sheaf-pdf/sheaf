# M5: Not in M5

Part of [M5: Scan & OCR](README.md).


- **Watched folders** (sheaf picking up whatever a copier drops into a
  folder, separating and filing it): M15's batch actions, on `sheaf
  separate`.
- **Scanners' own barcode and patch-code results** (TWAIN's extended
  image info, PaperStream's job separation): later, if a firm's scanner
  does better than sheaf's own reading. sheaf reads the codes in the
  pixels, the same on every scanner.
- **CCH target sheets by their own encoding:** CCH's sheets are its own
  barcodes, read by CCH Scan's engine. If their text is a standard
  symbology, a barcode rule reads them; a mapping of CCH's catalog codes
  to titles is not planned.
- **Sorting and naming pages by form** (W-2, 1099, K-1) without
  separators: that's classification, which the `tax-form-bookmarks`
  plugin does from text; CCH Scan's catalog and AutoFlow's data
  extraction are not planned.
- **Scanner settings beyond the common ones** (gamma, a driver's own
  dropout color, its own deskew and crop): not exposed. sheaf does its
  own cleanup, the same on every scanner.
- **A scanner preview scan** (a quick low-resolution flatbed pass to
  pick an area): later, if flatbed users ask.
- **Flatpak, Snap and the Microsoft Store:** sheaf ships bundles today
  ([`installer.md`](../topics/installer.md)). What each sandbox needs for scanners
  is under proposal 21, for when packaging comes.
