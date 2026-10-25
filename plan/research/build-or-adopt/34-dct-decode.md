# Build or adopt 34: DCT (JPEG) decode

Area: M11: Security and redaction · Part of [build or adopt](README.md)

- **Candidates:**
  - [`zune-jpeg`](https://crates.io/crates/zune-jpeg) 0.5.15
    (2026-03-26; 0.5.16-rc3 2026-10-19; MIT OR Apache-2.0 OR Zlib;
    etemesi254 alone; 140M downloads, as image's JPEG decoder).
    `forbid(unsafe_code)` unless its `x86`/`neon` SIMD features are on
    (`lib.rs:163`). `set_max_width`/`set_max_height`; output as RGB,
    YCbCr, CMYK or YCCK. **Already in the lockfile.**
  - [`jpeg-decoder`](https://crates.io/crates/jpeg-decoder) 0.3.2
    (2025-06-21; MIT OR Apache-2.0): "in maintenance mode as image-rs
    seeks to move over to the zune-jpeg crate". The only one with
    lossless JPEG (SOF3), rare in PDFs.
  - [`libjpeg-turbo-rs`](https://crates.io/crates/libjpeg-turbo-rs) 0.8.0
    (2026-07-28; MIT OR Apache-2.0): a pure-Rust port, first released
    2026-03. Too young.
- **Recommendation:** **adopt zune-jpeg directly** (no new crate). High.
