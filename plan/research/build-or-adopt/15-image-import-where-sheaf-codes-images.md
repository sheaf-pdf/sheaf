# Build or adopt 15: Image import, and where sheaf codes images today

Area: M5 (done): OCR, scan cleanup, image import · Part of [build or adopt](README.md)

- **Where:** M5 images-to-pdf, M7 export, M8 inserts (all done); M5
  intake slice 1 (G4 for black and white, JPEG for color and gray).
- **PDFium decodes every PDF image filter, but only to render.** sheaf
  never asks it for image data (no `PdfPageImageObject` use in
  `crates/`). PDFium's own codecs: libjpeg-turbo, an OpenJPEG fork, its
  own JBIG2 and CCITT decoders, zlib, and lcms2 for color.
- **sheaf decodes itself:**
  - image files in `sheaf_image::io`: JPEG through image (zune-jpeg
    0.5.15), plus sheaf's own SOF and EXIF parse for pass-through; PNG
    through png 0.18; TIFF through tiff 0.11.3, with `fax` for G4 and
    zune-jpeg for JPEG-in-TIFF; BMP, GIF and WebP through image;
  - PDF stream filters through lopdf: Flate, LZW, A85, AHx, RL and
    Brotli, with predictors and size caps, on flate2 and weezl;
  - inline images' ends, in the content lexer (`content/lex.rs:549-563`).
- **sheaf encodes itself:** PNG with png (`sheaf-image/src/encode.rs:14`);
  JPEG with image's encoder (`:29-44`); LZW TIFF with tiff (`:69`), with
  G4 TIFF directories sheaf writes; **CCITT G4 with `fax` 0.2's
  `Encoder`** (`:128-143`, B24); Flate with flate2
  (`sheaf-doc/src/images.rs:137-142`, `materialize/emit/mod.rs:860`,
  lopdf's `Stream::compress` at level 9).
- **Gap found:** tiff 0.11.3 refuses fax TIFFs other than G4 (entry 16,
  B25).
- **Recommendation:** keep this stack; fix B24 and B25.
