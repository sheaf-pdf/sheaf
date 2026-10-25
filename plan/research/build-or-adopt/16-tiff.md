# Build or adopt 16: TIFF

Area: M5 (done): OCR, scan cleanup, image import · Part of [build or adopt](README.md)

- [`tiff`](https://crates.io/crates/tiff) 0.11.3 (2026-02-10; MIT;
  image-rs), in use. It decodes planar data (`decoder/mod.rs:1147-1222`).
- **It doesn't decode G3 or Modified Huffman:** `Fax3` (3) and `Huffman`
  (2) go to `UnsupportedCompressionMethod`
  (`tiff-0.11.3/src/decoder/image.rs:604-620`). Fax software's TIFFs
  (Class F) are G3, so sheaf refuses them, and `io.rs` has no fallback.
  Found by reading the code, not reproduced with a file. B25.
- B15, on the `m5-intake` branch, lists the other kinds sheaf refuses.
  Since tiff decodes planar data, B15's planar case is sheaf's own
  refusal.
- **Recommendation:** **keep it, and add a G3 path through
  hayro-ccitt** (entry 38). High.
