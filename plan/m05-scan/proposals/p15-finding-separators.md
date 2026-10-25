# M5 proposal 15: Finding separators

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**Finding separators.**
- **In a document** (`separate::find(snap, views, rules, pages, p)`),
  as Find Blank Pages reads pages: each page drawn through its views
  with its content alone, in the background budget, in gray at **150
  dpi** (sheaf's QR modules are 4.4 pixels there; patch bars 12), or
  at **200 dpi** when a rule reads 1D codes (a 20 mil bar is 4
  pixels; thinner ones need the scan's own resolution), and its text
  read with its OCR text for keyword rules.
- **While scanning,** on the cleaned image itself, before OCR,
  reduced 2:1 by a box filter when it was scanned at 300 dpi (about
  5 ms).
- **What's read, in order:** patch bars along the four edges
  (sheaf's own reader, about 5 ms a page); then one barcode decode
  for sheaf's sheet and every rule's symbologies at once (rxing's
  `MultiFormatReader`, `TryHarder` on, its formats limited to QR and
  the rules' symbologies); then keywords. A code counts only if its
  text matches: `SHEAFSEP` for sheaf's sheet, the rule's pattern for
  others. One page is one separator at most, sheaf's sheet before
  any rule.
  - As built (slice 3): `codes::read(page, dpi, symbologies, accept,
    reader)` reads the patch bars, then the barcode, both every time.
    The page is binarized once (`BinaryBitmap` caches it) and decoded
    for each symbology asked for alone, in a fixed order (QR, Data
    Matrix, PDF417, Code 39, Code 128), each with `TryHarder`, until a
    code `accept` takes: the caller's text match ("a code counts only
    if its text matches"), so a code turned down lets the next
    symbology's decode run. One `MultiFormatReader` call over several
    formats walks the 2D ones in a `HashSet`'s order, random for each
    reader, so a page with two codes read as either one and sheaf's QR
    didn't come first; and any call stops at its first code, so one
    decode for the first code would let a vendor's QR code or a form's
    Code 39 id hide a rule's code every time. rxing gives one code a
    decode, so a second code of the same symbology is still hidden by
    the first (Slice notes, Left for later slices).
- **Never "every symbology, every code on the page":** the bench's
  whole-page multi-code search with all formats was 5–6 times slower
  and read junk retail codes (UPC-E, EAN-8, ITF, Codabar) off about
  15% of the corpus's real scans; limited to the separator
  symbologies, it read none.
- **Speed:** most pages aren't separators, so a page with none is
  what counts. In the bench (one thread, a 300 dpi Letter page of
  text, rules and a table), a page with no code took 44 ms decoded at
  150 dpi, 49 ms reduced from 300 (26 for a blank page), and 70 ms at
  200 dpi in black and white. A page with a code: 28–50 ms. rxing without `TryHarder` was
  twice as fast and missed every Data Matrix and small 1D code, so
  it isn't used.
- **Parallel:** renders on the engine pool's workers (Budget
  Background), decoding on a rayon pool. A long scan's search is
  then about its renders' time (gate in slice 5); the fallback, if
  renders are the bottleneck, is decoding a scanned page's own image
  instead of drawing it, as Stirling-PDF does.
