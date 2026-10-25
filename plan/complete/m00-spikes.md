# M0 spikes

```sh
cargo run -p sheaf-doc --example roundtrip [some.pdf]        # lopdf load → resave → reload
cargo run -p sheaf-render --example render_page [some.pdf]   # PDFium → target/spike/page-*.png
cargo run -p sheaf-app                                        # GPUI window
```

`sheaf-render` finds libpdfium via `$PDFIUM_LIB_DIR`, then
`vendor/pdfium/<os>-<arch>/`, then the system library. `sheaf-ocr` finds its
data via `$SHEAF_OCR_DATA_DIR`, then `ocr/` beside the binary, then
`vendor/ocr/`; `cargo test -p sheaf-ocr --features tesseract,oar -- --ignored`
runs the engines on a PDFium-rendered page.

`sheaf --timing <file.pdf>` prints open / first-render timings to stderr. On
WSL, keep test PDFs on the Linux filesystem: `/mnt/c` goes through 9P, which
is 5–10× slower for this workload and will mislead you about trends.

> Licensing note: there are two PDFium wrappers on crates.io. `pdfium-render`
> (MIT/Apache) is the one we use. `pdfium` (newinnovations) is GPL-3 and must
> not be added.
