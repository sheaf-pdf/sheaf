# Build or adopt 13: OCR engines

Area: M5 (done): OCR, scan cleanup, image import · Part of [build or adopt](README.md)

- **Need:** OCR to a text layer.
- **Where:** M5, done (`ROADMAP.md:111-115`); F21.
- **Today:** Tesseract 5, statically linked (`tesseract-rs` 0.4.0), and
  PP-OCRv5 on ONNX Runtime (`oar-ocr` 0.10.0), behind `OcrEngine`
  (`crates/sheaf-ocr/src/oar.rs:1`, `data.rs:101-111`).
- **Candidates:**
  - [`oar-ocr`](https://crates.io/crates/oar-ocr) 0.10.1 (2026-10-22,
    about every two weeks; Apache-2.0; GreatV alone; 192 stars). **It
    already supports PP-OCRv6**, in the 0.10.0 sheaf is on too (the
    `pp_ocrv6` preset and `pp-ocrv6_*` models,
    [README](https://github.com/greatv/oar-ocr)). It pins
    `ort = "=2.0.0-rc.13"`.
  - PaddleOCR released PP-OCRv6 on 2026-06-11
    ([paddleocr.ai](https://www.paddleocr.ai/latest/en/index.html)),
    claiming +5.1% recognition and +4.6% detection over
    PP-OCRv5_server (vendor figures). The v6 weights' license is
    unverified; probably Apache-2.0, like PaddleOCR's.
  - [`ocrs`](https://crates.io/crates/ocrs) 0.13.1 (2026-09-13; MIT OR
    Apache-2.0; Robert Knight; 1.9k stars; pure Rust on `rten`; builds
    for wasm). Its README: "currently in an early preview. Expect more
    errors than commercial OCR engines", and "recognizes the Latin
    alphabet only".
  - [`tesseract-rs`](https://crates.io/crates/tesseract-rs) 0.4.0
    (2026-07-31), current.
- **Fit:** ocrs isn't clearly better on tax forms; PP-OCRv6 might be.
- **Recommendation:** **keep both engines.** Try PP-OCRv6 small or tiny
  in the PaddleOCR engine, measured on the corpus scans (F21). High
  (keep); medium (v6 worth it).
