# Build or adopt 11: Barcode writing (sheaf's QR sheet)

Area: M5: separator sheets · Part of [build or adopt](README.md)

- **Need:** QR modules for sheaf's separator sheet, drawn as rectangles.
- **Where:** `m5-intake.md` proposal 13 (`:860-864`).
- **Today's plan:** "The modules come from the reader library's own QR
  encoder (rxing's `BitMatrix`)".
- **Candidates:** rxing; [`qrcode`](https://crates.io/crates/qrcode)
  0.14.1 (2024-07-05; MIT OR Apache-2.0; 23.5M downloads);
  [`fast_qr`](https://crates.io/crates/fast_qr) 0.14.0 (2026-09-01; MIT);
  [`qrcodegen`](https://crates.io/crates/qrcodegen) 1.8.0 (2022; MIT;
  Nayuki's reference); [`datamatrix`](https://crates.io/crates/datamatrix)
  0.3.3 (2026-05-31). All pure Rust.
- **Fit:** rxing covers it with no new dependency. M10's "Barcode fields:
  not planned" (`m10.md:2055`) means nothing else needs a writer.
- **Recommendation:** **adopt** rxing's encoder; `qrcode` is the
  fallback. High.
