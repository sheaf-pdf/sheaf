# Build or adopt: the milestones and the backlog, crate by crate

Status: **audit, 2026-10-23.** For each need in the milestones and the
backlog, whether sheaf should build it or adopt a crate that already
does it well. It informs each milestone's kickoff. It changes no plan
by itself: the plan lines it would change are listed under
[Changes to make](changes-to-make.md), each with who applies it.

- **The question** (Greg, 2026-10-23): "we should probably audit our
  milestones and backlog for whether these are things that we should
  build from scratch internally, or if there are high quality crates
  out there which already provide what we need."
- **Copyleft (GPL, LGPL, AGPL), Greg's rule:** cite it and study its
  behavior as prior art. Never copy, translate or link it.
- **sheaf is MIT OR Apache-2.0.** Permissive dependencies are fine. Data
  files (fonts, CMaps, ICC profiles, models) need their licenses checked
  too. MPL-2.0 is file-level copyleft: linking is allowed, but sheaf must
  offer the source of the MPL files it ships. That's a cost, named where
  it arises.
- **How it was done:** four researchers took an area each (fonts and
  text; security and signatures; images and scanning; PDF core and
  infrastructure). Crate facts come from the crates.io API, docs.rs,
  GitHub and the crates' published sources (downloaded and read, never
  built in a checkout), all on 2026-10-23. Download counts are all-time
  unless a line says otherwise. "Unverified" marks what couldn't be
  checked; nothing is guessed. Throwaway spikes ran outside every
  checkout: the B5 digest check (security) and a Flate comparison
  (images).
- **Line numbers** are on origin/main `8cdc589` (#30), except the M5 plan,
  which is on the `m5-intake` branch at `4481573`.
- **Confidence** is the researcher's: high, medium or low.

## Summary


- **Adopt more, build less, than the plans say.**
  - Fonts: skrifa and read-fonts now read Type 1 and bare CFF as well as
    TrueType, which covers every program a PDF embeds (entry 25).
  - CMaps: hayro-cmap carries the predefined CMaps (entry 26).
  - Images: hayro's CCITT, JBIG2 and JPEG 2000 decoders, with zune-jpeg
    and jpeg-encoder (entries 33–38).
  - Signatures: RustCrypto's formats crates, rustls-webpki and the
    platform key crates (entries 44–51).
  - Redaction geometry: i_overlay and kurbo (entries 31–32).
- **Build where no crate fits,** and say so plainly:
  - the PDF object layer: no crate replaces what sheaf uses of lopdf,
    so F13 stands (entry 1);
  - PDF signing, LTV and PAdES (entry 54);
  - re-subsetting the bare CFF and Type 1 programs PDFs embed (entry 62);
  - linearization and object-stream packing (entries 66–67);
  - PDF/A and PDF/UA rules (entries 68, 82), with veraPDF as the test
    tool, never linked;
  - scanner bindings and patch codes (entries 5, 6, 12).
- **PDFium can't be M11's image decoder.** Its bitmaps lose the color
  space, bit depth and palette, and its decoded data stops at the image
  filters (entry 33, decision D5).
- **subsetter always renumbers glyphs.** With `/Identity`, that breaks
  the plan's glyph-ID rule for the Noto fallback. A `/CIDToGIDMap`
  stream, from each glyph's number in the whole font to subsetter's,
  keeps the rule (added 2026-10-25; entry 17, D1).
- **23% of the corpus's real-world signatures are BER.** The stable
  cms 0.2.3 reads none of them; 0.3.0-rc.0 reads all of them, but the
  rest of RustCrypto's 0.3 round isn't published (entry 44, D2).
- **Gaps in the plans:** the macOS keychain in M12 (D8); a script font
  for typed signatures (entry 56); Symbol and ZapfDingbats substitutes
  for M14's embed-all profile (D10); the field editor handing a PDF's
  fonts to CoreText or DirectWrite (D9); a PDF/A file's XMP claim on
  sanitize (D12); JBIG2 globals shared across images (D5).
- **Bugs found** (now in the backlog): G4 truncates images wider than
  65,535 px (B24); fax TIFFs are refused (B25); the Flate backend
  depends on which crates are built (B26); nothing checks advisories or
  licenses (B27), which let wasmtime sit behind a critical advisory
  (PR #33); Flatpak printing may lack `lp` (B28); search misses
  ligatures, compatibility forms and NFD text (B29).
- **lopdf upstream moved fast this month:** about half of the weekend's
  planned pull requests are already merged by others (entry 2).
- **M5's crate survey still holds:** no maintained SANE or TWAIN crate,
  still no patch-code crate.

## Summary table


One row per entry below. "Where" names the milestone and slice, or the
backlog id. "D" points to a decision for Greg.

| # | Need | Where | Recommendation | Crate and version | License | Confidence |
|---|---|---|---|---|---|---|
| [1](01-pdf-object-layer.md) | The PDF object layer | F13, F15 | build in-house, from lopdf's MIT code | lopdf 0.46.0; hayro-syntax 0.8.0 as model and oracle | MIT; Apache-2.0 OR MIT | high |
| [2](02-lopdf-fixes-upstream.md) | lopdf fixes upstream | F13 | build on upstream: send only what's missing | lopdf main `d0b3abf` | MIT | high |
| [3](03-writing-pdf-sheaf-s-serializer-against.md) | Writing PDF | M10.5 (built); M14; M17 | build in-house (settled) | pdf-writer 0.15.0, krilla 0.8.2 as references | MIT OR Apache-2.0 | high |
| [4](04-flate-one-backend-every-build.md) | One Flate backend | every save; B26; D4 | adopt zlib-rs explicitly | flate2 1.1.10 + zlib-rs 0.6.8 | zlib-rs: Zlib | high (one); medium (which) |
| [5](05-sane-linux.md) | SANE scanning | M5 | build in-house | own bindings on libloading 0.9.0 | ISC | high |
| [6](06-twain-windows-with-32-bit-worker.md) | TWAIN, 32- and 64-bit | M5; D17 | build in-house | own bindings from `twain.h` | TWAIN DSM: LGPL-2.1 | high |
| [7](07-wia-2-0-windows.md) | WIA 2.0 | M5 | adopt | windows 0.62.2 | MIT OR Apache-2.0 | high |
| [8](08-macos-imagecapturecore.md) | macOS scanning | M5 ask 4 | adopt, when scheduled | objc2-image-capture-core 0.3.2 | Zlib OR Apache-2.0 OR MIT | medium |
| [9](09-escl.md) | eSCL | not planned | undecided (not planned) | escl 0.3.0, mdns-sd 0.21.6 | MIT OR Apache-2.0 | n/a |
| [10](10-barcode-reading.md) | Barcode reading | M5 separators; F16 | adopt behind a trait | rxing 0.9.3; zxing-cpp 0.5.3 second | Apache-2.0 | high |
| [11](11-barcode-writing.md) | Barcode writing | M5 proposal 13 | adopt | rxing 0.9.3; qrcode 0.14.1 fallback | Apache-2.0 | high |
| [12](12-kodak-patch-codes.md) | Kodak patch codes | M5 proposal 14 | build in-house | none exists | n/a | high |
| [13](13-ocr-engines.md) | OCR engines | M5 (done); F21 | adopt (keep both) | tesseract-rs 0.4.0; oar-ocr 0.10.0 | oar-ocr: Apache-2.0 | high (keep); medium (v6) |
| [14](14-scan-cleanup.md) | Scan cleanup | M5 (done) | build in-house (keep) | imageproc 0.27.0 | MIT | high |
| [15](15-image-import-where-sheaf-codes-images.md) | Image import and export | M5, M7, M8 (done); B24, B25 | adopt (keep) | image 0.25.10, png 0.18, tiff 0.11.3, fax 0.2.7 | permissive | n/a |
| [16](16-tiff.md) | TIFF | M5 import; B25 | adopt (keep), G3 through hayro-ccitt | tiff 0.11.3; hayro-ccitt 0.4.0 | MIT; MIT OR Apache-2.0 | high |
| [17](17-noto-fallback-glyph-id-preserving-type0.md) | Noto fallback subset | M10 slice 7; D1 | adopt subsetter behind a `/CIDToGIDMap` stream (2026-10-25); else build in-house on read-fonts | subsetter 0.2.6; read-fonts 0.46.0 or skera 0.9.0 otherwise | MIT OR Apache-2.0 | medium |
| [18](18-appearance-layout-comb-cells-auto-size.md) | Field appearance layout | M10 slices 6, 11 | build in-house | none needed | n/a | high |
| [19](19-unicode-basics-nfc-grapheme-clusters-right.md) | NFC, graphemes, RTL notice | M10 slice 7 | adopt | unicode-normalization 0.1.26, unicode-segmentation 1.13.3, unicode-bidi 0.3.18 | (MIT OR Apache-2.0) AND Unicode-3.0; MIT OR Apache-2.0 | high |
| [20](20-field-editor-s-painting.md) | Field editor painting | M10 slice 11; D9 | build on GPUI's `paint_glyph` | gpui-pre 0.3.9 | Apache-2.0 | medium |
| [21](21-bundled-fonts-their-licenses.md) | Bundled fonts | M10 slices 7, 11 | adopt, unmodified | Noto Sans v2.015; Liberation 2.1.5 | OFL-1.1 | high |
| [22](22-csv-tab-delimited-data.md) | CSV and tab-delimited | M10 slice 17 | adopt | csv 1.4.0 | Unlicense OR MIT | high |
| [23](23-fdf-xfdf.md) | FDF and XFDF | M10 slice 16 | build in-house on quick-xml | quick-xml 0.41 (0.42.0 latest) | MIT | high |
| [24](24-form-scripting.md) | Form scripting | M10 slices 18–22 | adopt boa as a wasm guest; build AF | boa_engine 0.22.0 | Unlicense OR MIT | high |
| [25](25-font-programs-outlines-metrics-boxes.md) | Font programs | M11 slices 3, 5; M10; M13; M14 | adopt behind sheaf's own module | skrifa 0.49.0, read-fonts 0.46.0 | MIT OR Apache-2.0 | high |
| [26](26-cmaps-predefined-cjk-cmaps-cid-unicode.md) | Predefined CMaps | M11 slices 3, 5; M13 | build on hayro-cmap | hayro-cmap 0.1.0 | Apache-2.0 OR MIT; data BSD-3 (Adobe) | medium |
| [27](27-standard-14-fonts-metrics.md) | Standard 14 metrics | M11 slice 3; M10 slice 6 | build in-house (keep) | Adobe's AFMs | Adobe's AFM notice | high |
| [28](28-outlines-redacted-glyphs.md) | Outlines of redacted glyphs | M11 slice 5; M14; D11 | build in-house on read-fonts | read-fonts 0.46.0 | MIT OR Apache-2.0 | medium |
| [29](29-standard-security-handler-read-r2-r6.md) | Standard security handler | M11 slices 1–2; F13 | build on RustCrypto (as today) | aes 0.9.3, cbc/ecb 0.2.1, md-5 0.11.0, sha2 0.11 | MIT OR Apache-2.0 | high |
| [30](30-redaction-hit-tests-glyphs-image-areas.md) | Redaction hit tests | M11 slices 5–7, 9 | build in-house in `geom` | rstar 0.13.0 only if measured | MIT OR Apache-2.0 | high |
| [31](31-clipping-paths-partly-under-mark.md) | Clipping paths under marks | M11 slice 6; D6 | adopt i_overlay directly | i_overlay 9.1.0; clipper2-rust 1.2.0 to compare | MIT OR Apache-2.0; BSL-1.0 | medium |
| [32](32-curve-flattening-stroke-expansion.md) | Flattening, stroke expansion | M11 slice 6 | adopt | kurbo 0.13.1 | Apache-2.0 OR MIT | high |
| [33](33-pdf-image-samples-image-s-own.md) | PDF image samples | M11 slice 6; M13; M14; M5; D5 | build on lopdf, zune-jpeg and hayro's codecs | zune-jpeg 0.5.15; hayro-ccitt 0.4.0, hayro-jbig2 0.3.1, hayro-jpeg2000 0.4.1 | MIT OR Apache-2.0 | high (shape); medium (JBIG2) |
| [34](34-dct-decode.md) | JPEG decode | M11 slice 6 | adopt | zune-jpeg 0.5.15 | MIT OR Apache-2.0 OR Zlib | high |
| [35](35-dct-encode.md) | JPEG encode | M11; M13; M14 | adopt | jpeg-encoder 0.7.1 | (MIT OR Apache-2.0) AND IJG | medium-high |
| [36](36-jpx-decode.md) | JPEG 2000 decode | M11; M14 | adopt (decode; no encoder) | hayro-jpeg2000 0.4.1 | MIT OR Apache-2.0 | high (decode); medium (no encoder) |
| [37](37-jbig2-decode.md) | JBIG2 decode | M11 | adopt (decode; never encode) | hayro-jbig2 0.3.1 | MIT OR Apache-2.0 | medium |
| [38](38-ccitt-g3-g4.md) | CCITT G3 and G4 | M11; B24; B25 | adopt: hayro-ccitt decodes, fax encodes | hayro-ccitt 0.4.0; fax 0.3.0 | MIT OR Apache-2.0; MIT | high |
| [39](39-png-files-flate-image-streams.md) | PNG and Flate images | M11; M14 | build on flate2 | png 0.18.1; oxipng 10.2.1 only if measured | MIT OR Apache-2.0; MIT | medium |
| [40](40-color-icc-profiles-output-intents.md) | ICC color, output intents | M11; M13; M14; D14 | adopt | moxcms 0.9.1 (0.8.1 locked) | BSD-3-Clause OR Apache-2.0 | medium |
| [41](41-xmp-sanitize-pdf-pdf-ua.md) | XMP metadata | M11 slice 9; M14; M17; B8; D12 | build in-house on quick-xml (D12) | quick-xml 0.41; xmp-writer 0.3.3 | MIT; MIT OR Apache-2.0 | medium (the sections split) |
| [42](42-javascript-actions-scripts-stripping.md) | Stripping actions and scripts | M11 slice 9; M10 | build in-house | quick-xml for XFA | MIT | high |
| [43](43-passwords-memory-remembering-them.md) | Passwords in memory | M11 slices 1–2; M12 | adopt (keep zeroize; secrecy in M12) | zeroize 1.9.1, secrecy 0.10.3 | Apache-2.0 OR MIT | high |
| [44](44-cms-signeddata-parse-build-verify.md) | CMS SignedData | M12 slices 2, 3, 5; B5; D2 | build on cms 0.3 | cms 0.3.0-rc.0, der 0.8, x509-cert 0.3.0 | Apache-2.0 OR MIT | medium-high |
| [45](45-signature-primitives.md) | Signature primitives | M12 slices 2–3; B5; D13 | adopt RustCrypto to verify; ring to sign | rsa 0.9.10, p256/p384 0.14.0, ecdsa 0.17.0, ed25519-dalek 3.0.0; ring 0.17.14 | MIT OR Apache-2.0; BSD-3-Clause; Apache-2.0 AND ISC | medium |
| [46](46-x-509-parsing.md) | X.509 parsing | M12 slices 2–5 | adopt | x509-cert 0.3.0 | Apache-2.0 OR MIT | high |
| [47](47-chain-validation-document-signatures.md) | Chain validation | M12 slices 3, 5 | build on rustls-webpki, behind a trait | rustls-webpki 0.103.16 | ISC | medium |
| [48](48-pkcs-12-files.md) | PKCS#12 files | M12 slice 2 | adopt, once on the published cms 0.3 | p12-keystore 0.4.1 | MIT/Apache-2.0 | medium |
| [49](49-pkcs-11-tokens.md) | PKCS#11 tokens | M12 slice 4 | adopt | cryptoki 0.12.1 | Apache-2.0 | high |
| [50](50-windows-certificate-store-cng-signing.md) | Windows store and CNG | M12 slice 4 | build on the windows crate | windows 0.62.2 | MIT OR Apache-2.0 | medium-high |
| [51](51-macos-keychain.md) | macOS keychain | M12 slice 4; D8 | adopt | security-framework 3.7.0 | MIT OR Apache-2.0 | high |
| [52](52-rfc-3161-timestamps.md) | RFC 3161 timestamps | M12 slice 5; D13 | build on x509-tsp 0.2 and cms 0.3 | x509-tsp 0.1.0 (0.2 unpublished); tsp-ltv 0.6.0 as oracle | Apache-2.0 OR MIT; BSD-2-Clause | medium |
| [53](53-ocsp-crl-fetch-parse-check.md) | OCSP and CRL | M12 slices 3, 5 | build on x509-ocsp 0.3 and x509-cert | x509-ocsp 0.2.1 (0.3 unpublished) | Apache-2.0 OR MIT | medium |
| [54](54-ltv-pades-levels.md) | LTV and PAdES | M12 slices 3, 5, 6 | build in-house | none fits; pyHanko 0.38.0 as oracle | (pyHanko: MIT) | high |
| [55](55-trust-anchors-verification.md) | Trust anchors | M12 slice 3; D13 | undecided | the OS stores (entries 50–51) | n/a | low |
| [56](56-script-font-typed-signatures.md) | Script font for typed signatures | M12 slice 1; D13 | undecided | not chosen | OFL or Apache, to check | low |
| [57](57-certificate-encryption-after-m12.md) | Certificate encryption | after M12; M11 slice 1 | build on cms and RustCrypto | cms 0.3, des 0.9.0, rc2 0.9.0 | cms: Apache-2.0 OR MIT | medium |
| [58](58-glyph-coverage-editing-existing-embedded-font.md) | Glyph coverage in a subset | M13 | build on skrifa | skrifa 0.49.0 | MIT OR Apache-2.0 | high |
| [59](59-shaping-bidi-reordering.md) | Shaping and bidi | M13; RTL fields | adopt | harfrust 0.15.0; unicode-bidi 0.3.18 | MIT; MIT OR Apache-2.0 | high |
| [60](60-block-reflow-line-breaking.md) | Block reflow, line breaks | M13 | build in-house on icu_segmenter | icu_segmenter 2.4.0; unicode-linebreak 0.1.5 lighter | Unicode-3.0; Apache-2.0 | medium |
| [61](61-font-matching-installed-fonts-fallback.md) | Font matching | M13; M10 later; M14 | build matching in-house; adopt fontdb | fontdb 0.24.0 (0.23 compiled); fontique 0.12.0 if needed | MIT; Apache-2.0 OR MIT | medium |
| [62](62-re-subsetting-fonts-already-file.md) | Re-subsetting embedded fonts | M14 optimize | build in-house on read-fonts | read-fonts 0.46.0 | MIT OR Apache-2.0 | medium |
| [63](63-pdf-preflight-font-checks-embedding-standard.md) | PDF/A font checks; embed-all | M14; D10 | adopt Foxit's substitutes; build the checks on skrifa | FoxitSymbol.pfb, FoxitDingbats.pfb | BSD-3 | medium |
| [64](64-image-downsampling.md) | Image downsampling | M14 optimize | adopt, after measuring | fast_image_resize 6.1.0 | MIT OR Apache-2.0 | medium |
| [65](65-recompression-lossless-flate.md) | Recompression | M14 optimize | adopt zlib-rs at level 9; "maximum" undecided | zlib-rs 0.6.8; zopfli 0.8.4, libdeflater 1.26.1 to measure | Zlib; Apache-2.0 | medium |
| [66](66-object-stream-packing.md) | Object-stream packing | M14; M10.5 §20 | build in-house in `serialize/` | lopdf's `object_stream.rs` as reference | MIT | high |
| [67](67-linearization-b3.md) | Linearization; B3 | M14; B3 | build in-house | qpdf 12.4.2 as reference and check | Apache-2.0 | high (no crate); medium (size) |
| [68](68-pdf-conversion-validation-preflight-lite.md) | PDF/A convert and validate | M14; D3; D14 | build in-house; veraPDF in tests | veraPDF 1.30.1 (tool); Arlington model; Compact-ICC-Profiles | GPLv3+ or MPLv2+ (not linked); Apache-2.0; CC0-1.0 | medium |
| [69](69-structure-trees-tags.md) | Structure trees and tags | M14; M17 | build in-house | krilla, pdf-writer as references | n/a | high |
| [70](70-compare-visual-diff.md) | Visual diff | M14 compare | build on PDFium renders and imageproc | blazediff 6.0.1 behind a trait, or a pixelmatch port | MIT; ISC | medium |
| [71](71-compare-text-diff.md) | Text diff | M14 compare | adopt | similar 3.2.0; imara-diff 0.2.0 if a benchmark says | Apache-2.0 | high (images); medium (core) |
| [72](72-compare-side-by-side-report.md) | Side-by-side report | M14 compare | build in-house | none needed | n/a | high |
| [73](73-batch-actions-sheaf-batch-optional-rhai.md) | Batch actions; macros | M15 | build in-house; macros undecided (Rhai if wanted) | rhai 1.27.0 | MIT OR Apache-2.0 | medium |
| [74](74-watched-folders.md) | Watched folders | M15 | as entry 88 | notify 8.2.0 | CC0-1.0 | medium |
| [75](75-mcp-server.md) | MCP server | M16 | adopt, pinned exactly | rmcp 3.7.0 | Apache-2.0 | high |
| [76](76-acp-client.md) | ACP client | agent panel (Pending) | adopt, pinned exactly | agent-client-protocol 3.5.0 | Apache-2.0 | high |
| [77](77-json-schema-ops-commands.md) | JSON Schema | M16; M15; F5 | adopt (keep) | schemars 1.2.3 (1.2.2 locked) | MIT | high |
| [78](78-local-transport.md) | Local transport | M16 decisions 1–3 | build on tokio | tokio (1.53.2 locked) | MIT | medium |
| [79](79-same-user-checks-agent-socket.md) | Same-user check, Unix | M16 | adopt tokio's `peer_cred` | tokio 1.54.1 | MIT | high |
| [80](80-same-user-checks-windows.md) | Same-user check, Windows | M16; D16 | build on the windows crate, over tokio's pipes | windows; interprocess 2.4.6 alternative | MIT OR Apache-2.0; 0BSD OR Apache-2.0 | medium |
| [81](81-app-with-screen-reader.md) | Screen reader support | M17 | adopt, through gpui-pre | gpui-pre 0.3.9 (accesskit 0.24.1 locked) | MIT OR Apache-2.0 | medium-high |
| [82](82-pdf-ua-validation.md) | PDF/UA validation | M17; F9 | build in-house; veraPDF in tests | veraPDF 1.30 (`ua1`, `ua2`) | not linked | medium |
| [83](83-text-structure-reading-order-actualtext-alt.md) | Reading order, ActualText, alt text | M17; M7; Pending | build in-house | oar-ocr (adopted) | Apache-2.0 | medium |
| [84](84-spell-check.md) | Spell check | Pending; D15 | adopt behind a trait | spellbook 0.4.2; platform checkers; SCOWL en_US | MPL-2.0; permissive | medium |
| [85](85-search-case-folding-unicode-equivalence.md) | Search equivalence | B29; Pending | build on unicode-normalization | unicode-normalization 0.1.26 | (MIT OR Apache-2.0) AND Unicode-3.0 | low |
| [86](86-export-markdown-agent-panel-s-markdown.md) | Markdown export and rendering | Pending; agent panel | build in-house (export); adopt (panel) | pulldown-cmark 0.13.5 | MIT | medium (export); high (panel) |
| [87](87-search-across-files.md) | Search across files | Pending | build in-house on `Session` | walkdir 2.5.0 | Unlicense OR MIT | high (the CLI) |
| [88](88-file-watching-file-contention.md) | File watching | Pending (file contention); M15 | build on `FileStamp` polling; adopt notify later | notify 8.2.0 | CC0-1.0 | medium |
| [89](89-xlsx-or-out.md) | XLSX | nothing yet | undecided | rust_xlsxwriter 0.101.0; calamine 0.37.0 | MIT OR Apache-2.0; MIT | high (nothing now) |
| [90](90-ml-runtimes.md) | ML runtimes | M5 (done); Pending; M17 | adopt (keep ort); a switch undecided | ort 2.0.0-rc.13 | MIT OR Apache-2.0 | high (keep); low (switch) |
| [91](91-rendering-pdfium-hayro-as-second-renderer.md) | Rendering; hayro | Pending; F17 | adopt (keep PDFium); hayro an oracle | pdfium-render 0.9.4; hayro 0.8.0 | MIT OR Apache-2.0 | medium |
| [92](92-watch-jpeg-xl-pdf.md) | JPEG XL in PDF | watch | undecided (watch) | jxl-oxide 0.12.6 | MIT OR Apache-2.0 | n/a |
| [93](93-b5-signature-already-broken-still-asks.md) | B5's digest check | B5; F9 | build on cms 0.3.0-rc.0 | cms 0.3.0-rc.0, der 0.8, sha1 0.11, sha2 0.11 | Apache-2.0 OR MIT | high |
| [94](94-checking-what-sheaf-writes-against-arlington.md) | Checks against Arlington | F22 | build in-house (test-only); since published as pdf-lint (2026-10-26) | the Arlington PDF Model; pdf-lint 0.1.0 | Apache-2.0 | n/a |
| [95](95-printing.md) | Printing | M7 (done); B28 | build in-house (keep) | printers 2.3.0 rejected | MIT | medium |
| [96](96-single-instance-progress.md) | Single instance, progress | M9; PR #32 | adopt (Greg's crates) | instance-handoff 0.1.0-alpha.1; in-progress 0.1.0 | MIT OR Apache-2.0 | n/a |
| [97](97-settings.md) | Settings | M15 | adopt (keep); move to 1.x | toml 0.9.12 (1.1.8 latest) | MIT OR Apache-2.0 | high |
| [98](98-cli.md) | The CLI | done | adopt (keep) | clap 4.6.7 | n/a | high |
| [99](99-fuzzing-property-tests.md) | Fuzzing, property tests | done; F13 | adopt (keep) | libfuzzer-sys 0.4, proptest 1.12.0 | (MIT OR Apache-2.0) AND NCSA | high |
| [100](100-wasmtime-security-patches.md) | wasmtime advisories | plugins; forms guest; PR #33 | adopt the patch release | wasmtime 49.0.4 | n/a | n/a (done in #33) |
| [101](101-needs-that-need-no-crate.md) | Needs with no crate | Pending; M13; F1–F4 | build in-house | none needed | n/a | n/a |
| [102](102-table-extraction.md) | Table extraction | prior art | adopt csv; XLSX is entry 89 | csv 1.4.0 | Unlicense OR MIT | n/a |

## More

- [Decisions for Greg](decisions.md): D1–D17, open and ruled
- [The findings by area](by-area.md): each entry in its area, with the area's notes
- [Audit notes](audit-notes.md): what was already acted on, what the build already compiles, and the backlog entries the audit added
- [Changes to make, by file](changes-to-make.md): plan edits the audit proposed, to apply at each kickoff
