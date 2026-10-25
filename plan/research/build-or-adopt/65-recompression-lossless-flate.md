# Build or adopt 65: Recompression and lossless Flate

Area: M14: Document processing · Part of [build or adopt](README.md)

- **Need:** images recompressed to JPEG (lossy, by choice), bilevel to
  G4, the rest to Flate; Flate streams recompressed harder; PDF/A-1
  conversion turning LZW streams into Flate and JPX images into JPEG or
  Flate (PDF/A-1 permits neither LZW nor JPX).
- **Candidates:** the encoders above; for Flate, zlib-rs 0.6.8 and
  miniz_oxide 0.9.1 (entry 4), [`zopfli`](https://crates.io/crates/zopfli)
  0.8.4 (2026-10-11; Apache-2.0; pure Rust; very slow, for maximum
  compression; in the lockfile only as a build-time dependency of
  tesseract-rs) and [`libdeflater`](https://crates.io/crates/libdeflater)
  1.26.1 (2026-09-17; Apache-2.0 bindings to C libdeflate, MIT; level
  12).
- **Fit:** whether zopfli or libdeflate earns its time on a 7,000-page
  binder is unmeasured.
- **Recommendation:** **default to zlib-rs at level 9.** A "maximum"
  setting is undecided: measure size gained per second spent on the
  corpus. Medium.
