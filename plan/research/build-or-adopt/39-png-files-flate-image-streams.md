# Build or adopt 39: PNG files, and Flate image streams

Area: M11: Security and redaction · Part of [build or adopt](README.md)

- **Candidates:**
  - [`png`](https://crates.io/crates/png) 0.18.1 (2026-02-14; MIT OR
    Apache-2.0; image-rs; `forbid(unsafe_code)`): in use.
  - [`oxipng`](https://crates.io/crates/oxipng) 10.2.1 (2026-09-02; MIT;
    an active org), for PNG files only. It **always** depends on
    `libdeflater`, which is C (libdeflate, MIT). A PDF Flate image can go
    through it treated as a PNG: its IDAT zlib stream is a valid
    `/FlateDecode` with `/Predictor 15`, as long as oxipng's color-type,
    bit-depth and palette reductions are off.
- **Recommendation:** for PDF images, **build on flate2** plus sheaf's own
  PNG predictor choice. Measure an oxipng route (or `png`'s adaptive
  filter, read back the same way) only if M14 wants every last byte.
  Medium.
