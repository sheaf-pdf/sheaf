# Build or adopt 33: PDF image samples, in the image's own color space

Area: M11: Security and redaction · Part of [build or adopt](README.md)

The images section's detail; the security section deferred to it and
added two notes, kept below.
- **Need:** an image XObject's or inline image's samples in its own color
  space, bit depth and `/Decode`, so filling a region and re-encoding
  changes nothing else.
- **Where:** M11 slice 6; the same layer serves M13 (image edit, replace,
  crop), M14 (downsampling, recompression, PDF/A re-encoding), slice 6's
  check, and M5's separator fallback, "decoding a scanned page's own
  image instead of drawing it" (`m5-intake.md:934`). D5.
- **Today's plan:** "A partly covered image is decoded, its pixels under
  the mark are replaced with the fill color, and it's encoded again.
  PDFium decodes, since it handles every filter. Bilevel images become
  CCITT G4 (`sheaf-image`), JPEGs become JPEG again, and the rest Flate"
  (`m11.md:150-157`). Slice 6's check: "pixels under marks are the fill
  color in the image data itself" (`:503-505`). Unbuilt.
- **Why PDFium doesn't fit** (its source):
  - [`FPDFImageObj_GetBitmap`](https://pdfium.googlesource.com/pdfium/+/HEAD/fpdfsdk/fpdf_editimg.cpp)
    `Realize`s or `ConvertTo`s the image into 8bpp gray or BGR/BGRx/BGRA.
    1-bit images come back 8-bit, paletted ones BGR, masks are ignored,
    and CMYK, ICCBased, Separation, DeviceN, Indexed and 16-bit images
    come back converted. Re-encoding those pixels turns every redacted
    image into DeviceRGB or DeviceGray: print colors shift, spot colors
    are lost, and a PDF/A file can stop conforming.
  - `FPDFImageObj_GetImageDataDecoded` goes through `PDF_DataDecode`,
    which stops at the last filter it takes for an image decoder ("assume
    it's an image decoder",
    [`fpdf_parser_decode.cpp`](https://pdfium.googlesource.com/pdfium/+/HEAD/core/fpdfapi/parser/fpdf_parser_decode.cpp)),
    so for DCT, JPX, JBIG2 and CCITT it returns still-encoded bytes.
  - Every call also goes through sheaf's global PDFium lock.
- **Candidates:** one per filter (entries 34–38). lopdf covers the stream
  filters with caps (`vendor/lopdf/src/object.rs:893-898`; predictors at
  `:1119-1150`).
  - `hayro-syntax` 0.8.0 wires exactly this set behind its `images`
    feature (zune-jpeg, hayro-ccitt, hayro-jbig2, hayro-jpeg2000), but
    its `filter::Filter::apply` is crate-private, so sheaf can't call it
    on lopdf objects. Its `filter/*.rs` (2,132 lines, MIT OR Apache-2.0)
    is the reference for the PDF quirks: DCT `/ColorTransform` and Adobe
    APP14; JPEGs whose headers give wrong dimensions; CCITT
    `DecodeParms`; JBIG2 globals; JPX `SMaskInData`.
- **Fit:**
  - Every crate is pure Rust, safe at the crate level, permissive, and
    lets sheaf read the dimensions before decoding, to cap the work.
    Each builds for wasm: hayro's demo viewer runs in the browser; the
    others are `no_std` or pure Rust.
  - pdf_oxide (MIT OR Apache-2.0, 1.35M downloads) uses nearly the same
    stack: `fax` ^0.3, `jpeg-decoder`, `hayro-jbig2`, `hayro-jpeg2000`,
    optional `lcms2` and `fast_image_resize`.
  - **How often each filter occurs** (a byte search for each name over
    `/home/greg/repos/pdf-corpus`, `pool/` excluded):

    | Category | DCT | CCITT | JBIG2 | JPX | LZW |
    |---|---|---|---|---|---|
    | `scans/` (709 files) | 464 | 150 | 127 | 52 | 2 |
    | `forms/` (4,392) | 1,521 | 189 | 17 | 89 | 37 |
    | `parsing/` (5,253) | 1,060 | 286 | 93 | 95 | 93 |

  - **Security's notes:** replace pixels in raw samples in the image's
    own color space and depth (CMYK, Indexed, 16-bit, ICC), never in a
    rendered BGRA bitmap. Re-encoding a JPEG loses quality over the whole
    image, not only under the mark, so the report says which images were
    re-encoded.
- **Recommendation:** **build on lopdf, zune-jpeg, hayro-ccitt,
  hayro-jbig2 and hayro-jpeg2000,** in one `sheaf-image` module (pixels
  in, pixels out) with a small decoder trait, so PDFium's bitmap can be
  a cross-check in tests. PDFium stays the renderer that checks the
  output. High on the shape; medium on hayro-jbig2 ("under
  development").
