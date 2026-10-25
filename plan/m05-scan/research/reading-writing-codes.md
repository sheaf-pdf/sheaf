# M5 research: Reading and writing codes

Part of [M5: Scan & OCR](../README.md).


- **Rust readers.** rxing 0.9.3 (Apache-2.0, a pure-Rust port of ZXing
  3.5.1 with zxing-cpp's QR and Data Matrix readers; every common 1D
  and 2D symbology; builds for wasm). zxing-cpp's Rust wrapper 0.5.3
  (Apache-2.0; the C++20 library, built with CMake). rqrr and bardecoder
  read QR only. zbar-rust is LGPL-2.1 with no Data Matrix or PDF417;
  zedbar, a Rust port of ZBar, is LGPL-3.0, awkward to link statically
  into a Rust binary. rxing's default features pin `image = "=0.25.8"` (sheaf
  is on 0.25.10): it's used with `default-features = false` and the
  features `decoders, encoders, encoding_rs, oned, qrcode, datamatrix,
  pdf417` (as built: PDF417 is a feature of its own in 0.9.3, and ask 9
  makes it a rule's symbology; `multi_barcode_readers` stays off: a page
  is decoded for one code of each symbology asked for).
  zxing-cpp's author's benchmark (so perhaps partial) found rxing slower
  and less thorough on QR, and reading codes in pages with none when
  every format was on.
- **The bench** (a scratch crate, one thread, EPYC 7302P): synthetic
  300 dpi Letter pages of text, rules and a table with one code (Code
  128, Code 39, QR, Data Matrix, PDF417, each at two sizes), in 17
  degradations (turns of 90° and 180°, skew to 3°, noise, blur, salt and
  pepper, low contrast, JPEG at 40, black and white at 300, 200 and 150
  dpi, and an "office" mix of blur, skew, noise and 1-bit at 200 or 150).

  | Reader | Read (of 170) | No code, 150 dpi | No code, 200 dpi 1-bit | Real scans with junk reads |
  |---|---|---|---|---|
  | rxing, one decode, separator symbologies, `TryHarder` | 149 | 44 ms (49 reduced from 300) | 70 ms | 0 of 150 |
  | rxing, whole-page multi-code, all formats | 149 | 66 ms | 94 ms | about 22 of 150 |
  | rxing without `TryHarder` | 73 | 17 ms | 28 ms | — |
  | zxing-cpp, separator symbologies | 159 | 30 ms | 47 ms | 0 of 150 |
  | rqrr (QR only) | 25 of 34 | 51 ms | 93 ms | — |

  Both main readers read QR and Data Matrix at 0.34 mm modules in every
  1-bit 200 and 150 dpi case; 10 mil 1D codes and small PDF417 broke at
  150 dpi. A vector page of each symbology at three sizes, rasterized
  and read back, says 1D codes need 20 mil bars at 200 dpi in 1-bit.
- **Real pages carry codes.** Of 200 form pages and 150 scans from the
  corpus, the separator-limited readers found real codes on 8 forms and
  2 scans: Code 39 form ids, a Data Matrix `$REG32608$`, a QR saying "DO
  NOT READ", Code 128s.
- **Writers.** rxing encodes every symbology it reads (one module a
  pixel with no margin); `qrcode` 0.14.1 and `datamatrix` 0.3.3 (whose
  `Bitmap::path()` is a vector outline) are the alternatives;
  `barcoders` is 1D only. rxing's and barcoders' Code 39 is 2:1, below
  USS-39's 2.2–3.0 for thin bars, should sheaf ever print one.
- **Patch codes.** Kodak's A-61599 (2006; Alliance hosts a copy):
  wide bars 0.20 in, narrow bars and spaces 0.08 in, 0.80 in in all, at
  least 0.75 in long (2 in for Kodak's Patch-Reader scanners; as built,
  sheaf's reader asks 0.6 in, for a skewed or clipped sheet), at least
  0.25 in from the edges and other print, within 3.75 in of the leading
  edge; bars under 20% reflectance on paper over 65%; no
  photocopies. Kodak's "Patches for Printing" sheets (Letter and A4)
  carry the bars on all four edges plus a Code 39 `PATCHn`. No
  open-source detector was found (zxing-cpp has none; NAPS2 reads only
  the Code 39 text).
  A prototype in the bench (75 dpi, Otsu, lines across each edge band,
  bars classed wide over 0.14 in) read 204 of 204 synthetic pages and
  60 of 60 renders of Kodak's sheets, found nothing on 204 synthetic
  and 350 real pages without codes, in about 5 ms a page. Bars
  perpendicular to the leading edge (Kodak's i280 layout) aren't read.
- **Drivers' own readers.** TWAIN has patch and barcode detection
  (`TWEI_PATCHCODE`, `TWEI_BARCODETEXT`; QR among its barcode types,
  Data Matrix not); SANE's fujitsu, kodak and canon_dr backends expose
  none. Reading the pixels is the one way that works everywhere.
