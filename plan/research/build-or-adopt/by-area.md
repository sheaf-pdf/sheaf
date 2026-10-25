# Build or adopt: the findings by area

Part of [build or adopt](README.md).

Each entry gives the need, where it's needed, today's plan, the
candidates, the fit, and the recommendation with its confidence. Where
two sections covered the same need, the entry says whose detail stands.

### Cross-cutting: the PDF object layer, writing, and Flate

- [1. The PDF object layer (F13)](01-pdf-object-layer.md)
- [2. lopdf fixes upstream (the weekend's pull requests)](02-lopdf-fixes-upstream.md)
- [3. Writing PDF: sheaf's serializer against pdf-writer and krilla](03-writing-pdf-sheaf-s-serializer-against.md)
- [4. Flate: one backend in every build](04-flate-one-backend-every-build.md)

### M5: scanner intake (the `m5-intake` branch)

- [5. SANE on Linux](05-sane-linux.md)
- [6. TWAIN on Windows, with a 32-bit worker](06-twain-windows-with-32-bit-worker.md)
- [7. WIA 2.0 on Windows](07-wia-2-0-windows.md)
- [8. macOS ImageCaptureCore (not in M5)](08-macos-imagecapturecore.md)
- [9. eSCL (not planned)](09-escl.md)

### M5: separator sheets

- [10. Barcode reading](10-barcode-reading.md)
- [11. Barcode writing (sheaf's QR sheet)](11-barcode-writing.md)
- [12. Kodak patch codes](12-kodak-patch-codes.md)

### M5 (done): OCR, scan cleanup, image import

- [13. OCR engines](13-ocr-engines.md)
- [14. Scan cleanup](14-scan-cleanup.md)
- [15. Image import, and where sheaf codes images today](15-image-import-where-sheaf-codes-images.md)
- [16. TIFF](16-tiff.md)

### M10: Forms

- [17. The Noto fallback: a glyph-ID-preserving Type0 subset, late-bound per save](17-noto-fallback-glyph-id-preserving-type0.md)
- [18. Appearance layout: comb cells, auto size, wrapping](18-appearance-layout-comb-cells-auto-size.md)
- [19. Unicode basics: NFC, grapheme clusters, the right-to-left notice](19-unicode-basics-nfc-grapheme-clusters-right.md)
- [20. The field editor's painting (slice 11)](20-field-editor-s-painting.md)
- [21. The bundled fonts and their licenses](21-bundled-fonts-their-licenses.md)
- [22. CSV and tab-delimited data (slice 17)](22-csv-tab-delimited-data.md)
- [23. FDF and XFDF (slice 16)](23-fdf-xfdf.md)
- [24. Form scripting (slices 18–22)](24-form-scripting.md)

### M11: Security and redaction

- [25. Font programs: outlines, metrics and boxes](25-font-programs-outlines-metrics-boxes.md)
- [26. CMaps: predefined CJK CMaps and CID → Unicode](26-cmaps-predefined-cjk-cmaps-cid-unicode.md)
- [27. The standard 14 fonts' metrics](27-standard-14-fonts-metrics.md)
- [28. Outlines of redacted glyphs (M11's leak; M14's re-subsetting)](28-outlines-redacted-glyphs.md)
- [29. The Standard security handler: read R2–R6, keep on save, write AES-256](29-standard-security-handler-read-r2-r6.md)
- [30. Redaction hit tests: glyphs, image areas, annotations](30-redaction-hit-tests-glyphs-image-areas.md)
- [31. Clipping paths partly under a mark (polygon booleans)](31-clipping-paths-partly-under-mark.md)
- [32. Curve flattening and stroke expansion](32-curve-flattening-stroke-expansion.md)
- [33. PDF image samples, in the image's own color space](33-pdf-image-samples-image-s-own.md)
- [34. DCT (JPEG) decode](34-dct-decode.md)
- [35. DCT (JPEG) encode](35-dct-encode.md)
- [36. JPX (JPEG 2000) decode](36-jpx-decode.md)
- [37. JBIG2 decode](37-jbig2-decode.md)
- [38. CCITT G3 and G4](38-ccitt-g3-g4.md)
- [39. PNG files, and Flate image streams](39-png-files-flate-image-streams.md)
- [40. Color: ICC profiles and output intents](40-color-icc-profiles-output-intents.md)
- [41. XMP: sanitize (M11), PDF/A and PDF/UA (M14, M17)](41-xmp-sanitize-pdf-pdf-ua.md)
- [42. JavaScript, actions and scripts: stripping](42-javascript-actions-scripts-stripping.md)
- [43. Passwords in memory, and remembering them](43-passwords-memory-remembering-them.md)

### M12: Signatures

No Rust crate signs or verifies PDFs in a form sheaf can build on. What
exists is whole-PDF libraries (pdf_oxide, oxidize-pdf), a GPL crate
(pdf_signer), crates on old lopdf (pdf_signing, underskrift), and one
published on 2026-10-23 (`pades`). M12 builds on RustCrypto's formats
crates, rustls-webpki for the RFC 5280 core, and the platform key
crates. Nothing here runs in a plugin (signing and decrypting are
host-only), so wasm builds weren't tried.

**The corpus, measured** (over `../pdf-corpus/signatures/`, with a
script and the B5 spike):
- 593 CMS blobs reached through `/ByteRange`. **138 use BER indefinite
  lengths,** all in `safedocs/`, the real-world set.
- Signature algorithms: 569 rsaEncryption (522 sha256WithRSA, 178
  sha1WithRSA, 72 sha512WithRSA, 10 sha384WithRSA), 7 EC, 5 Ed25519, 2
  RSASSA-PSS.
- Digests: 451 sha256, 150 sha1, 37 sha512, 4 md5, 2 sha384, 10 GOST.
- Attributes: 178 carry a signature timestamp, 315 signingCertificateV2,
  144 Adobe's `adbe-revocationInfoArchival`. 99 files have a `/DSS`.
- SubFilters: adbe.pkcs7.detached, ETSI.CAdES.detached and ETSI.RFC3161;
  no adbe.x509.rsa_sha1, no adbe.pkcs7.sha1.

- [44. CMS SignedData: parse, build, verify](44-cms-signeddata-parse-build-verify.md)
- [45. Signature primitives (verify; sign with in-memory keys)](45-signature-primitives.md)
- [46. X.509 parsing](46-x-509-parsing.md)
- [47. Chain validation for document signatures](47-chain-validation-document-signatures.md)
- [48. PKCS#12 files](48-pkcs-12-files.md)
- [49. PKCS#11 tokens](49-pkcs-11-tokens.md)
- [50. Windows certificate store and CNG signing](50-windows-certificate-store-cng-signing.md)
- [51. macOS keychain (missing from the plan)](51-macos-keychain.md)
- [52. RFC 3161 timestamps](52-rfc-3161-timestamps.md)
- [53. OCSP and CRL: fetch, parse, check](53-ocsp-crl-fetch-parse-check.md)
- [54. LTV (DSS and VRI) and PAdES levels](54-ltv-pades-levels.md)
- [55. Trust anchors for verification](55-trust-anchors-verification.md)
- [56. A script font for typed signatures (not in the plans)](56-script-font-typed-signatures.md)
- [57. Certificate encryption (`/Adobe.PubSec`), after M12](57-certificate-encryption-after-m12.md)

### M13: Content editing

- [58. Glyph coverage, and editing in an existing embedded font](58-glyph-coverage-editing-existing-embedded-font.md)
- [59. Shaping and bidi reordering](59-shaping-bidi-reordering.md)
- [60. Block reflow and line breaking](60-block-reflow-line-breaking.md)
- [61. Font matching, installed fonts and fallback](61-font-matching-installed-fonts-fallback.md)

### M14: Document processing

- [62. Re-subsetting fonts already in the file (optimize)](62-re-subsetting-fonts-already-file.md)
- [63. PDF/A and preflight font checks; embedding the standard 14](63-pdf-preflight-font-checks-embedding-standard.md)
- [64. Image downsampling](64-image-downsampling.md)
- [65. Recompression and lossless Flate](65-recompression-lossless-flate.md)
- [66. Object-stream packing](66-object-stream-packing.md)
- [67. Linearization (Fast Web View) and B3](67-linearization-b3.md)
- [68. PDF/A conversion and validation; preflight-lite](68-pdf-conversion-validation-preflight-lite.md)
- [69. Structure trees and tags (M14 and M17)](69-structure-trees-tags.md)
- [70. Compare: visual (pixel) diff](70-compare-visual-diff.md)
- [71. Compare: text diff](71-compare-text-diff.md)
- [72. Compare: the side-by-side report](72-compare-side-by-side-report.md)

### M15: Automation

- [73. Batch actions, `sheaf batch`, and the optional Rhai front end](73-batch-actions-sheaf-batch-optional-rhai.md)
- [74. Watched folders (from M5's plan)](74-watched-folders.md)

### M16: Agent tools (MCP), and the agent panel (Pending)

- [75. The MCP server (`sheaf mcp`)](75-mcp-server.md)
- [76. The ACP client (the agent panel)](76-acp-client.md)
- [77. JSON Schema for ops and commands](77-json-schema-ops-commands.md)
- [78. The local transport (relay socket, Windows pipe)](78-local-transport.md)
- [79. Same-user checks on the agent socket (Unix)](79-same-user-checks-agent-socket.md)
- [80. Same-user checks on Windows (named pipes or AF_UNIX)](80-same-user-checks-windows.md)

### M17: Accessibility (and M7's text with layout)

- [81. The app with a screen reader (AccessKit through gpui-pre)](81-app-with-screen-reader.md)
- [82. PDF/UA validation](82-pdf-ua-validation.md)
- [83. Text structure: reading order, ActualText, alt text, text with layout](83-text-structure-reading-order-actualtext-alt.md)

### Pending

- [84. Spell check](84-spell-check.md)
- [85. Search: case folding and Unicode equivalence](85-search-case-folding-unicode-equivalence.md)
- [86. Export to Markdown, and the agent panel's Markdown](86-export-markdown-agent-panel-s-markdown.md)
- [87. Search across files](87-search-across-files.md)
- [88. File watching and file contention](88-file-watching-file-contention.md)
- [89. XLSX in or out](89-xlsx-or-out.md)
- [90. ML runtimes (UVDoc in use, DocRes pending)](90-ml-runtimes.md)
- [91. Rendering: PDFium, and hayro as a second renderer](91-rendering-pdfium-hayro-as-second-renderer.md)
- [92. Watch: JPEG XL in PDF](92-watch-jpeg-xl-pdf.md)

### Backlog items

- [93. B5: a signature already broken still asks](93-b5-signature-already-broken-still-asks.md)
- [94. Checking what sheaf writes against the Arlington PDF Model](94-checking-what-sheaf-writes-against-arlington.md)

### Infrastructure that's built (confirmed)

- [95. Printing (`sheaf-print`)](95-printing.md)
- [96. Single instance and progress (Greg's crates)](96-single-instance-progress.md)
- [97. Settings (toml)](97-settings.md)
- [98. The CLI (clap)](98-cli.md)
- [99. Fuzzing and property tests](99-fuzzing-property-tests.md)
- [100. wasmtime security patches (plugins and the forms guest)](100-wasmtime-security-patches.md)

### Needs with no crate, and notes

- [101. Needs that need no crate](101-needs-that-need-no-crate.md)
- [102. Table extraction (pdfplumber-core, prior art)](102-table-extraction.md)
#### Not covered here
- Code signing of sheaf's own binaries, and the installer's SHA-256
  checks (`plan/topics/code-signing.md`, `plan/topics/installer.md`): packaging.
- Plugin sandbox limits (F11): the plugins area.
