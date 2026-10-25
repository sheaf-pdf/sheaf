# Build or adopt 38: CCITT G3 and G4

Area: M11: Security and redaction · Part of [build or adopt](README.md)

- **Candidates:**
  - [`hayro-ccitt`](https://crates.io/crates/hayro-ccitt) 0.4.0
    (2026-10-04; MIT OR Apache-2.0; no dependencies;
    `forbid(unsafe_code)`; `no_std`): G3 1D, G3 2D and G4 (T.4 and T.6).
    Its `DecodeSettings` (columns, rows, `end_of_block`,
    `rows_are_byte_aligned`) mirror PDF's `CCITTFaxDecode` parameters.
    Decoding only.
  - [`fax`](https://crates.io/crates/fax) 0.3.0 (2026-07-13; sheaf is on
    0.2.7; MIT; s3bk of pdf-rs; `deny(unsafe_code)`; 69M downloads, as
    tiff's G4 codec): G4 encoder and decoder, a G3 1D decoder. **0.3
    moved widths from `u16` to `u32`** (`decode_g4(…, width: u32, …)`).
- **sheaf's G4 encoder** is `sheaf_image::encode::g4`
  (`crates/sheaf-image/src/encode.rs:128-143`), which calls
  `enc.encode_line(pels, w as u16)` (`:136`). An image wider than 65,535
  px (a 600 dpi scan over 109 in wide) is encoded at the truncated
  width, while `/Columns` and the TIFF's `ImageWidth` keep the true one:
  the output is corrupt. Nothing in `sheaf_image::io` caps width. B24.
- **Recommendation:** decode with hayro-ccitt (every `K`, and the
  byte-align and end-of-block flags), for G3 TIFFs too (B25); keep
  encoding with `fax`, bumped to 0.3, or refuse widths it can't take
  (B24). High.
