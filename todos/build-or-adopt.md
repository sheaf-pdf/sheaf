# Build or adopt: the milestones and the backlog, crate by crate

Status: **audit, 2026-10-23.** For each need in the milestones and the
backlog, whether sheaf should build it or adopt a crate that already
does it well. It informs each milestone's kickoff. It changes no plan
by itself: the plan lines it would change are listed under
[Changes to make](#changes-to-make-by-file), each with who applies it.

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
- **subsetter always renumbers glyphs,** so the plan's glyph-ID rule
  for the Noto fallback rules it out (entry 17, D1).
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
| 1 | The PDF object layer | F13, F15 | build in-house, from lopdf's MIT code | lopdf 0.46.0; hayro-syntax 0.8.0 as model and oracle | MIT; Apache-2.0 OR MIT | high |
| 2 | lopdf fixes upstream | F13 | build on upstream: send only what's missing | lopdf main `d0b3abf` | MIT | high |
| 3 | Writing PDF | M10.5 (built); M14; M17 | build in-house (settled) | pdf-writer 0.15.0, krilla 0.8.2 as references | MIT OR Apache-2.0 | high |
| 4 | One Flate backend | every save; B26; D4 | adopt zlib-rs explicitly | flate2 1.1.10 + zlib-rs 0.6.8 | zlib-rs: Zlib | high (one); medium (which) |
| 5 | SANE scanning | M5 | build in-house | own bindings on libloading 0.9.0 | ISC | high |
| 6 | TWAIN, 32- and 64-bit | M5; D17 | build in-house | own bindings from `twain.h` | TWAIN DSM: LGPL-2.1 | high |
| 7 | WIA 2.0 | M5 | adopt | windows 0.62.2 | MIT OR Apache-2.0 | high |
| 8 | macOS scanning | M5 ask 4 | adopt, when scheduled | objc2-image-capture-core 0.3.2 | Zlib OR Apache-2.0 OR MIT | medium |
| 9 | eSCL | not planned | undecided (not planned) | escl 0.3.0, mdns-sd 0.21.6 | MIT OR Apache-2.0 | n/a |
| 10 | Barcode reading | M5 separators; F16 | adopt behind a trait | rxing 0.9.3; zxing-cpp 0.5.3 second | Apache-2.0 | high |
| 11 | Barcode writing | M5 proposal 13 | adopt | rxing 0.9.3; qrcode 0.14.1 fallback | Apache-2.0 | high |
| 12 | Kodak patch codes | M5 proposal 14 | build in-house | none exists | n/a | high |
| 13 | OCR engines | M5 (done); F21 | adopt (keep both) | tesseract-rs 0.4.0; oar-ocr 0.10.0 | oar-ocr: Apache-2.0 | high (keep); medium (v6) |
| 14 | Scan cleanup | M5 (done) | build in-house (keep) | imageproc 0.27.0 | MIT | high |
| 15 | Image import and export | M5, M7, M8 (done); B24, B25 | adopt (keep) | image 0.25.10, png 0.18, tiff 0.11.3, fax 0.2.7 | permissive | n/a |
| 16 | TIFF | M5 import; B25 | adopt (keep), G3 through hayro-ccitt | tiff 0.11.3; hayro-ccitt 0.4.0 | MIT; MIT OR Apache-2.0 | high |
| 17 | Noto fallback subset | M10 slice 7; D1 | build in-house on read-fonts | read-fonts 0.46.0; skera 0.9.0 alternative | MIT OR Apache-2.0 | medium |
| 18 | Field appearance layout | M10 slices 6, 11 | build in-house | none needed | n/a | high |
| 19 | NFC, graphemes, RTL notice | M10 slice 7 | adopt | unicode-normalization 0.1.26, unicode-segmentation 1.13.3, unicode-bidi 0.3.18 | (MIT OR Apache-2.0) AND Unicode-3.0; MIT OR Apache-2.0 | high |
| 20 | Field editor painting | M10 slice 11; D9 | build on GPUI's `paint_glyph` | gpui-pre 0.3.9 | Apache-2.0 | medium |
| 21 | Bundled fonts | M10 slices 7, 11 | adopt, unmodified | Noto Sans v2.015; Liberation 2.1.5 | OFL-1.1 | high |
| 22 | CSV and tab-delimited | M10 slice 17 | adopt | csv 1.4.0 | Unlicense OR MIT | high |
| 23 | FDF and XFDF | M10 slice 16 | build in-house on quick-xml | quick-xml 0.41 (0.42.0 latest) | MIT | high |
| 24 | Form scripting | M10 slices 18–22 | adopt boa as a wasm guest; build AF | boa_engine 0.22.0 | Unlicense OR MIT | high |
| 25 | Font programs | M11 slices 3, 5; M10; M13; M14 | adopt behind sheaf's own module | skrifa 0.49.0, read-fonts 0.46.0 | MIT OR Apache-2.0 | high |
| 26 | Predefined CMaps | M11 slices 3, 5; M13 | build on hayro-cmap | hayro-cmap 0.1.0 | Apache-2.0 OR MIT; data BSD-3 (Adobe) | medium |
| 27 | Standard 14 metrics | M11 slice 3; M10 slice 6 | build in-house (keep) | Adobe's AFMs | Adobe's AFM notice | high |
| 28 | Outlines of redacted glyphs | M11 slice 5; M14; D11 | build in-house on read-fonts | read-fonts 0.46.0 | MIT OR Apache-2.0 | medium |
| 29 | Standard security handler | M11 slices 1–2; F13 | build on RustCrypto (as today) | aes 0.9.3, cbc/ecb 0.2.1, md-5 0.11.0, sha2 0.11 | MIT OR Apache-2.0 | high |
| 30 | Redaction hit tests | M11 slices 5–7, 9 | build in-house in `geom` | rstar 0.13.0 only if measured | MIT OR Apache-2.0 | high |
| 31 | Clipping paths under marks | M11 slice 6; D6 | adopt i_overlay directly | i_overlay 9.1.0; clipper2-rust 1.2.0 to compare | MIT OR Apache-2.0; BSL-1.0 | medium |
| 32 | Flattening, stroke expansion | M11 slice 6 | adopt | kurbo 0.13.1 | Apache-2.0 OR MIT | high |
| 33 | PDF image samples | M11 slice 6; M13; M14; M5; D5 | build on lopdf, zune-jpeg and hayro's codecs | zune-jpeg 0.5.15; hayro-ccitt 0.4.0, hayro-jbig2 0.3.1, hayro-jpeg2000 0.4.1 | MIT OR Apache-2.0 | high (shape); medium (JBIG2) |
| 34 | JPEG decode | M11 slice 6 | adopt | zune-jpeg 0.5.15 | MIT OR Apache-2.0 OR Zlib | high |
| 35 | JPEG encode | M11; M13; M14 | adopt | jpeg-encoder 0.7.1 | (MIT OR Apache-2.0) AND IJG | medium-high |
| 36 | JPEG 2000 decode | M11; M14 | adopt (decode; no encoder) | hayro-jpeg2000 0.4.1 | MIT OR Apache-2.0 | high (decode); medium (no encoder) |
| 37 | JBIG2 decode | M11 | adopt (decode; never encode) | hayro-jbig2 0.3.1 | MIT OR Apache-2.0 | medium |
| 38 | CCITT G3 and G4 | M11; B24; B25 | adopt: hayro-ccitt decodes, fax encodes | hayro-ccitt 0.4.0; fax 0.3.0 | MIT OR Apache-2.0; MIT | high |
| 39 | PNG and Flate images | M11; M14 | build on flate2 | png 0.18.1; oxipng 10.2.1 only if measured | MIT OR Apache-2.0; MIT | medium |
| 40 | ICC color, output intents | M11; M13; M14; D14 | adopt | moxcms 0.9.1 (0.8.1 locked) | BSD-3-Clause OR Apache-2.0 | medium |
| 41 | XMP metadata | M11 slice 9; M14; M17; B8; D12 | build in-house on quick-xml (D12) | quick-xml 0.41; xmp-writer 0.3.3 | MIT; MIT OR Apache-2.0 | medium (the sections split) |
| 42 | Stripping actions and scripts | M11 slice 9; M10 | build in-house | quick-xml for XFA | MIT | high |
| 43 | Passwords in memory | M11 slices 1–2; M12 | adopt (keep zeroize; secrecy in M12) | zeroize 1.9.1, secrecy 0.10.3 | Apache-2.0 OR MIT | high |
| 44 | CMS SignedData | M12 slices 2, 3, 5; B5; D2 | build on cms 0.3 | cms 0.3.0-rc.0, der 0.8, x509-cert 0.3.0 | Apache-2.0 OR MIT | medium-high |
| 45 | Signature primitives | M12 slices 2–3; B5; D13 | adopt RustCrypto to verify; ring to sign | rsa 0.9.10, p256/p384 0.14.0, ecdsa 0.17.0, ed25519-dalek 3.0.0; ring 0.17.14 | MIT OR Apache-2.0; BSD-3-Clause; Apache-2.0 AND ISC | medium |
| 46 | X.509 parsing | M12 slices 2–5 | adopt | x509-cert 0.3.0 | Apache-2.0 OR MIT | high |
| 47 | Chain validation | M12 slices 3, 5 | build on rustls-webpki, behind a trait | rustls-webpki 0.103.16 | ISC | medium |
| 48 | PKCS#12 files | M12 slice 2 | adopt, once on the published cms 0.3 | p12-keystore 0.4.1 | MIT/Apache-2.0 | medium |
| 49 | PKCS#11 tokens | M12 slice 4 | adopt | cryptoki 0.12.1 | Apache-2.0 | high |
| 50 | Windows store and CNG | M12 slice 4 | build on the windows crate | windows 0.62.2 | MIT OR Apache-2.0 | medium-high |
| 51 | macOS keychain | M12 slice 4; D8 | adopt | security-framework 3.7.0 | MIT OR Apache-2.0 | high |
| 52 | RFC 3161 timestamps | M12 slice 5; D13 | build on x509-tsp 0.2 and cms 0.3 | x509-tsp 0.1.0 (0.2 unpublished); tsp-ltv 0.6.0 as oracle | Apache-2.0 OR MIT; BSD-2-Clause | medium |
| 53 | OCSP and CRL | M12 slices 3, 5 | build on x509-ocsp 0.3 and x509-cert | x509-ocsp 0.2.1 (0.3 unpublished) | Apache-2.0 OR MIT | medium |
| 54 | LTV and PAdES | M12 slices 3, 5, 6 | build in-house | none fits; pyHanko 0.38.0 as oracle | (pyHanko: MIT) | high |
| 55 | Trust anchors | M12 slice 3; D13 | undecided | the OS stores (entries 50–51) | n/a | low |
| 56 | Script font for typed signatures | M12 slice 1; D13 | undecided | not chosen | OFL or Apache, to check | low |
| 57 | Certificate encryption | after M12; M11 slice 1 | build on cms and RustCrypto | cms 0.3, des 0.9.0, rc2 0.9.0 | cms: Apache-2.0 OR MIT | medium |
| 58 | Glyph coverage in a subset | M13 | build on skrifa | skrifa 0.49.0 | MIT OR Apache-2.0 | high |
| 59 | Shaping and bidi | M13; RTL fields | adopt | harfrust 0.15.0; unicode-bidi 0.3.18 | MIT; MIT OR Apache-2.0 | high |
| 60 | Block reflow, line breaks | M13 | build in-house on icu_segmenter | icu_segmenter 2.4.0; unicode-linebreak 0.1.5 lighter | Unicode-3.0; Apache-2.0 | medium |
| 61 | Font matching | M13; M10 later; M14 | build matching in-house; adopt fontdb | fontdb 0.24.0 (0.23 compiled); fontique 0.12.0 if needed | MIT; Apache-2.0 OR MIT | medium |
| 62 | Re-subsetting embedded fonts | M14 optimize | build in-house on read-fonts | read-fonts 0.46.0 | MIT OR Apache-2.0 | medium |
| 63 | PDF/A font checks; embed-all | M14; D10 | adopt Foxit's substitutes; build the checks on skrifa | FoxitSymbol.pfb, FoxitDingbats.pfb | BSD-3 | medium |
| 64 | Image downsampling | M14 optimize | adopt, after measuring | fast_image_resize 6.1.0 | MIT OR Apache-2.0 | medium |
| 65 | Recompression | M14 optimize | adopt zlib-rs at level 9; "maximum" undecided | zlib-rs 0.6.8; zopfli 0.8.4, libdeflater 1.26.1 to measure | Zlib; Apache-2.0 | medium |
| 66 | Object-stream packing | M14; M10.5 §20 | build in-house in `serialize/` | lopdf's `object_stream.rs` as reference | MIT | high |
| 67 | Linearization; B3 | M14; B3 | build in-house | qpdf 12.4.2 as reference and check | Apache-2.0 | high (no crate); medium (size) |
| 68 | PDF/A convert and validate | M14; D3; D14 | build in-house; veraPDF in tests | veraPDF 1.30.1 (tool); Arlington model; Compact-ICC-Profiles | GPLv3+ or MPLv2+ (not linked); Apache-2.0; CC0-1.0 | medium |
| 69 | Structure trees and tags | M14; M17 | build in-house | krilla, pdf-writer as references | n/a | high |
| 70 | Visual diff | M14 compare | build on PDFium renders and imageproc | blazediff 6.0.1 behind a trait, or a pixelmatch port | MIT; ISC | medium |
| 71 | Text diff | M14 compare | adopt | similar 3.2.0; imara-diff 0.2.0 if a benchmark says | Apache-2.0 | high (images); medium (core) |
| 72 | Side-by-side report | M14 compare | build in-house | none needed | n/a | high |
| 73 | Batch actions; macros | M15 | build in-house; macros undecided (Rhai if wanted) | rhai 1.27.0 | MIT OR Apache-2.0 | medium |
| 74 | Watched folders | M15 | as entry 88 | notify 8.2.0 | CC0-1.0 | medium |
| 75 | MCP server | M16 | adopt, pinned exactly | rmcp 3.7.0 | Apache-2.0 | high |
| 76 | ACP client | agent panel (Pending) | adopt, pinned exactly | agent-client-protocol 3.5.0 | Apache-2.0 | high |
| 77 | JSON Schema | M16; M15; F5 | adopt (keep) | schemars 1.2.3 (1.2.2 locked) | MIT | high |
| 78 | Local transport | M16 decisions 1–3 | build on tokio | tokio (1.53.2 locked) | MIT | medium |
| 79 | Same-user check, Unix | M16 | adopt tokio's `peer_cred` | tokio 1.54.1 | MIT | high |
| 80 | Same-user check, Windows | M16; D16 | build on the windows crate, over tokio's pipes | windows; interprocess 2.4.6 alternative | MIT OR Apache-2.0; 0BSD OR Apache-2.0 | medium |
| 81 | Screen reader support | M17 | adopt, through gpui-pre | gpui-pre 0.3.9 (accesskit 0.24.1 locked) | MIT OR Apache-2.0 | medium-high |
| 82 | PDF/UA validation | M17; F9 | build in-house; veraPDF in tests | veraPDF 1.30 (`ua1`, `ua2`) | not linked | medium |
| 83 | Reading order, ActualText, alt text | M17; M7; Pending | build in-house | oar-ocr (adopted) | Apache-2.0 | medium |
| 84 | Spell check | Pending; D15 | adopt behind a trait | spellbook 0.4.2; platform checkers; SCOWL en_US | MPL-2.0; permissive | medium |
| 85 | Search equivalence | B29; Pending | build on unicode-normalization | unicode-normalization 0.1.26 | (MIT OR Apache-2.0) AND Unicode-3.0 | low |
| 86 | Markdown export and rendering | Pending; agent panel | build in-house (export); adopt (panel) | pulldown-cmark 0.13.5 | MIT | medium (export); high (panel) |
| 87 | Search across files | Pending | build in-house on `Session` | walkdir 2.5.0 | Unlicense OR MIT | high (the CLI) |
| 88 | File watching | Pending (file contention); M15 | build on `FileStamp` polling; adopt notify later | notify 8.2.0 | CC0-1.0 | medium |
| 89 | XLSX | nothing yet | undecided | rust_xlsxwriter 0.101.0; calamine 0.37.0 | MIT OR Apache-2.0; MIT | high (nothing now) |
| 90 | ML runtimes | M5 (done); Pending; M17 | adopt (keep ort); a switch undecided | ort 2.0.0-rc.13 | MIT OR Apache-2.0 | high (keep); low (switch) |
| 91 | Rendering; hayro | Pending; F17 | adopt (keep PDFium); hayro an oracle | pdfium-render 0.9.4; hayro 0.8.0 | MIT OR Apache-2.0 | medium |
| 92 | JPEG XL in PDF | watch | undecided (watch) | jxl-oxide 0.12.6 | MIT OR Apache-2.0 | n/a |
| 93 | B5's digest check | B5; F9 | build on cms 0.3.0-rc.0 | cms 0.3.0-rc.0, der 0.8, sha1 0.11, sha2 0.11 | Apache-2.0 OR MIT | high |
| 94 | Checks against Arlington | F22 | build in-house (test-only) | the Arlington PDF Model | Apache-2.0 | n/a |
| 95 | Printing | M7 (done); B28 | build in-house (keep) | printers 2.3.0 rejected | MIT | medium |
| 96 | Single instance, progress | M9; PR #32 | adopt (Greg's crates) | instance-handoff 0.1.0-alpha.1; in-progress 0.1.0 | MIT OR Apache-2.0 | n/a |
| 97 | Settings | M15 | adopt (keep); move to 1.x | toml 0.9.12 (1.1.8 latest) | MIT OR Apache-2.0 | high |
| 98 | The CLI | done | adopt (keep) | clap 4.6.7 | n/a | high |
| 99 | Fuzzing, property tests | done; F13 | adopt (keep) | libfuzzer-sys 0.4, proptest 1.12.0 | (MIT OR Apache-2.0) AND NCSA | high |
| 100 | wasmtime advisories | plugins; forms guest; PR #33 | adopt the patch release | wasmtime 49.0.4 | n/a | n/a (done in #33) |
| 101 | Needs with no crate | Pending; M13; F1–F4 | build in-house | none needed | n/a | n/a |
| 102 | Table extraction | prior art | adopt csv; XLSX is entry 89 | csv 1.4.0 | Unlicense OR MIT | n/a |

## Decisions for Greg

Each has its options, recommended first. Where the sections left a
choice open, the pick is this memo's and says so.

### D1: The Noto fallback: keep the glyph-ID rule? (M10 slice 7)

The plan says any font sheaf embeds keeps each glyph's number from the
whole font (`content/font/mod.rs:35-38`; m10.md:1455-1460), and
m10.md:633 names subsetter. subsetter always renumbers. Entry 17.

1. **Keep the rule. Build a glyph-ID-preserving `glyf` subset in-house
   on read-fonts,** a port of M10.5's `gen.py` algorithm. Use skera with
   `SUBSET_FLAGS_RETAIN_GIDS` if it grows. It keeps codes writable before
   the subset is known, appearance digests independent of the save, and
   an Append's earlier subset valid. The cost: `loca` and `hmtx` at about
   15.5 KB each before Flate. Medium.
2. Drop the rule and adopt subsetter 0.2.6, the mature choice. The
   emitter then picks codes per output (two passes), and M10.5's `Font`
   docs change.

### D2: M12's RustCrypto line: wait for the cms 0.3 round, or pin pre-releases?

cms 0.2.3, the only stable release, reads none of the corpus's 138 BER
signatures. cms 0.3.0-rc.0 (2026-10-13) reads all of them. x509-ocsp
0.3, x509-tsp 0.2 and pkcs12 0.2 aren't published, and p12-keystore
0.4.1 can't share a build with cms 0.3.0-rc.0. Entries 44, 48, 93.

1. **Don't hold work for the round.** Build B5 on cms 0.3.0-rc.0 now,
   pinned exactly. At M12's kickoff, re-check the line: take the
   published round if it's out; otherwise pin the pre-release set M12
   needs, exactly. p12-keystore waits for the published cms 0.3.
   Medium-high.
2. Hold M12 until the round is published.
3. Stay on cms 0.2.3, or use openssl. No: 0.2.3 can't read a quarter of
   real signatures, and openssl puts a C parser in the path of untrusted
   input.

### D3: veraPDF in tests and CI: Java or Docker? (M14, M17, F9)

Nothing in Rust validates PDF/A or PDF/UA near veraPDF's depth.
veraPDF is GPLv3+ or MPLv2+, so it runs as a separate tool, never
linked. Entries 68, 82.

1. **Java plus the veraPDF 1.30 CLI** wherever the tiers run: a corpus
   tier over `conformance/` comparing sheaf's checks with veraPDF's JSON,
   and a tier that runs veraPDF on what sheaf converts. Each skips
   without it, as corpus tiers do without the corpus. In Linux CI, run it
   on sheaf's own written outputs. Medium.
2. The `verapdf/cli` Docker image. The newest tag a search showed was
   v1.28.2, possibly stale.
3. Rust-only checks, with no outside validator.

Also at M14's kickoff: whether sheaf validates other tools' files for
users. If so, veraPDF as an optional external tool or plugin, not a rule
engine written in Rust.

### D4: One Flate backend for the workspace

The shipped `sheaf` binary compresses with zlib-rs, because sheaf-app's
`zip` turns on `flate2/zlib-rs`. The `sheaf-cli` binary, per-crate test
runs and builds without plugins use miniz_oxide. The two give different
bytes at levels 1, 6 and 9. Entry 4, B26.

1. **zlib-rs, named once:** `flate2 = { version = "1", default-features
   = false, features = ["zlib-rs"] }` in the workspace, used by every
   crate. It's what ships today. Check whether anything replays a memory
   source across builds or versions and expects the same BlobId. High on
   one backend; medium on which.
2. miniz_oxide everywhere (flate2's default). zlib-rs has about 456
   `unsafe` in its `src/`; with miniz_oxide, the shipped app's output
   changes instead.
3. Leave it to feature unification.

### D5: M11's image decoding (slice 6)

PDFium can't decode for re-encoding: `FPDFImageObj_GetBitmap` converts to
BGR or gray, and `GetImageDataDecoded` stops at DCT, JPX, JBIG2 and
CCITT. Entries 33–38.

1. **One sample decoder in `sheaf-image`** (pixels in, pixels out, behind
   a small trait): lopdf for the stream filters, zune-jpeg for DCT, and
   hayro-ccitt, hayro-jbig2 and hayro-jpeg2000. PDFium stays the
   renderer that checks the output. High on the shape; medium on
   hayro-jbig2, "under development".
2. PDFium's bitmap: every redacted image becomes DeviceRGB or
   DeviceGray, spot colors are lost, and a PDF/A file can stop
   conforming.
3. C codecs (openjpeg-sys, turbojpeg): C parsing untrusted images, where
   the pure-Rust crates are safe at the crate level.

With it, three smaller asks:
- **Re-encoding:** JPEG with jpeg-encoder (CMYK, the source's tables, no
  APPn carried over; an IJG notice); JPX becomes Flate; JBIG2 becomes G4;
  JBIG2 is never written. Recommended, medium-high.
- **A JBIG2 globals stream another image shares** (15 of the corpus's
  127 JBIG2 scans share one): its symbols can include glyphs only the
  redacted area used. The section left it open. **This memo's pick:**
  re-encode every image that shares it and drop the stream, and say so
  in the report. The other option is to keep it, after deciding it
  leaks nothing.
- **The report** says which images were re-encoded, since a re-encoded
  JPEG loses quality over the whole image.

### D6: Clipping line art: i_overlay directly, and strokes (M11 slice 6)

m11.md:160-163 names "`geo` or `i_overlay` (licenses to check)". Both are
MIT OR Apache-2.0. geo 0.33.1 pins i_overlay `>=4.5.1, <4.6.0`, five
majors back. Entry 31.

1. **i_overlay 9.x directly**, pinned exactly, behind a one-function
   sheaf-doc module (path minus marks), with kurbo for flattening and
   strokes. clipper2-rust (BSL-1.0) runs beside it in the corpus tier as
   a differential check. Coordinates are checked (finite, bounded) and
   segment counts capped first, because the float API panics. Medium.
2. geo's `BooleanOps`: an old i_overlay and a GIS API, for no gain.

And strokes, which the section left for kickoff:
1. **This memo's pick: expand the stroke to its outline with kurbo**
   (joins, caps, miter limit, dashes), subtract the marks and fill the
   rest. It's exact. The cost: a stroke becomes a fill, which M13's
   editing then sees.
2. Clip the centerline (i_overlay's inverted string clip, or clipper2's
   open subjects) against marks grown by half the line width under the
   CTM and the miter limit. The stroke stays a stroke, with new butt
   ends at the cuts, but the growth has to be right for every join.

### D7: An advisory and license check in CI

sheaf has none: no `deny.toml`, nothing in `.github/`. M12 brings crypto
crates with live advisories (rsa: RUSTSEC-2023-0071, unpatched), and
this audit checked licenses by hand. B27.

1. **`cargo deny check advisories licenses`**, with a reviewed ignore
   list. Bans later. Small.
2. `cargo audit` (advisories only).
3. Keep checking by hand.

### D8: The macOS keychain in M12

M12 names the Windows store and PKCS#11 tokens (ROADMAP.md:397-398), not
the keychain, though macOS is a target platform. Entry 51.

1. **Add it,** on security-framework 3.7.0 (already in the lockfile):
   identities, `SecKeyCreateSignature`, trust settings. High.
2. macOS signs only with PKCS#12 files and tokens.

### D9: The field editor's painting (M10 slice 11)

The plan paints a field with no Liberation match "in that embedded font
if GPUI can load it" (m10.md:624-627). That hands untrusted font bytes
to CoreText or DirectWrite, C and C++ stacks with a history of font
CVEs, and registers the font for the process's lifetime. Entry 20.

1. **Paint sheaf's own glyph runs with GPUI's `Window::paint_glyph`**,
   from slice 6's layout on the bundled fonts, so the editor matches the
   appearance by construction. Never `add_fonts` a PDF's program: draw
   its skrifa outlines as GPUI paths, or fall back to Liberation Mono or
   Sans. Medium: `paint_glyph` with fonts from `add_fonts` is unverified
   on macOS and Windows, so spike it first.
2. The plan as written.

### D10: M14's embed-all substitutes for Symbol and ZapfDingbats

`Profile.fonts: EmbedAll` embeds a substitute for every standard 14 font
sheaf writes, ZapfDingbats check boxes included. Liberation covers only
Helvetica, Times and Courier. Entry 63.

1. **Foxit's FoxitSymbol.pfb and FoxitDingbats.pfb** (Type 1, BSD-3, the
   PDFium Authors), which PDFium draws for them and pdf.js ships. Medium.
2. Leave those two unembedded, and the profile fails on them.

### D11: Blank redacted glyphs' outlines in M11, not M14 (M11 slice 5)

Today a font subset in the redacted copy can show that a character
occurred, though not where, until M14 re-subsets (m11.md:562-564). Slice
5 already computes the codes "no longer used anywhere" for `/ToUnicode`
and `/Widths`. Entry 28.

1. **Blank their outlines in slice 5:** a zero-length `glyf` entry, an
   `endchar` charstring in CFF, the same under eexec in Type 1. Codes,
   widths and content streams don't change. That closes the documented
   leak, and M14 reuses it. Medium.
2. Keep the leak documented until M14.

### D12: XMP: one writer or two, and a PDF/A file's claim on sanitize

M11 slice 9 removes metadata, which voids a PDF/A or PDF/UA claim held
in XMP. M14 and M17 write XMP that keeps a file's other schemas. The
sections split: security picked xmp-writer for M11's fresh packet
(high), core picked quick-xml for M14's patching, warning that two
writers mean two code paths (medium). Entry 41.

- **The claim:**
  1. **Keep it:** write a minimal fresh packet with `pdfaid` and
     `pdfuaid`, Producer and ModifyDate.
  2. Say the claim goes.
- **The writer, this memo's reconciliation:**
  1. **One path on quick-xml** (read, patch, write) for both, with
     xmp-writer's schema coverage as the checklist. M14 has to patch
     existing packets anyway, and M11's packet is "keep only these,
     set those".
  2. xmp-writer for M11's fresh packet, and patching added in M14.

### D13: M12's other kickoff asks

Entries 45, 47, 52, 55, 56.
- **Signing with PKCS#12 keys:**
  1. **ring** (constant-time, already in sheaf-app's build) wrapped in a
     `signature::Signer` adapter for cms's builder.
  2. the rsa crate, whose Marvin advisory (RUSTSEC-2023-0071) is still
     unpatched. Fine for local use, but an audit tool would flag it.
- **Trust anchors:**
  1. **User-trusted certificates plus the OS stores** to start, the EU
     Trusted Lists later. Low confidence.
  2. Also the Adobe Approved Trust List, if third parties may use it
     (unverified).
- **Timestamps and revocation:**
  1. **x509-tsp 0.2 and x509-ocsp 0.3 once published, with a small
     synchronous, bounded HTTP client;** tsp-ltv as prior art and an
     oracle. Which client is open: the installer picked ureq
     (`todos/installer.md:81`), and hyper, tokio and rustls are already
     linked.
  2. Adopt tsp-ltv, if it moves to the 0.3 line and lets callers supply
     the transport.
- **Chain validation:**
  1. **rustls-webpki behind a trait,** with sheaf's PDF policy (EKU,
     signing time, DSS revocation, legacy algorithms) on top.
  2. synta-x509-verification (RFC 5280 §6, pluggable crypto), if it has
     matured by kickoff; it's young.
- **The typed-signature font:** pick an OFL or Apache script font, its
  license checked, embedded through M10 slice 7's subsetting.

### D14: The PDF/A output intent profile (M14)

The sections split. Core: take krilla's CC0 profiles from
Compact-ICC-Profiles. Images: moxcms (already built) can generate an sRGB
profile, so no third-party file is needed. Entries 40, 68.

1. **This memo's pick: the CC0 Compact-ICC-Profiles** (`sRGB-v4.icc`,
   `sRGB-v2-magic.icc`, the gray pair), which krilla ships and checks
   against veraPDF. Which part needs v2 or v4 is unverified; settle it
   with veraPDF at kickoff.
2. A profile generated by moxcms. moxcms stays either way, for
   converting fill colors into an image's color space.

### D15: Spell check engines (Pending)

Entry 84.
1. **Platform checkers on Windows and macOS, spellbook on Linux,** with
   SCOWL's en_US and a tax word list bundled; drop zspell. Medium.
2. spellbook everywhere, if firms want identical suggestions on every
   OS.

spellbook is MPL-2.0: sheaf must offer its source.

### D16: M16 on Windows: bridge threads or a per-user named pipe? (agent.md decision 2)

Entries 78, 80.
1. **tokio's named pipes** (first instance, reject remote clients, a
   user-only security descriptor), with the SID check before any data
   flows: about 50 lines on the windows crate, which no crate does.
   interprocess 2.4.6 if a synchronous API is wanted. Medium.
2. `uds_windows` plus the `SIO_AF_UNIX_GETPEERPID` ioctl, which keeps
   today's socket. Microsoft's documentation for the ioctl wasn't found.

### D17: Copyleft loaded at run time (M5)

The rule says never link copyleft. M5's plan ships the TWAIN DSM
(`TWAINDSM.dll`, LGPL-2.1) unmodified and loads it at run time
(`m5-intake.md:660-662`). A Flatpak would bundle sane-backends (GPL-2.0+
with the SANE exception), loaded through libsane. Entries 5, 6.

1. **Allowed:** loading an unmodified, separately shipped library at run
   time isn't copying, translating or linking sheaf against it. Ship
   its license text and a source offer, as M5 plans.
2. Not allowed: users install TWAIN DSM and sane-backends themselves,
   and the Flatpak has no scanning.

**Ruled 2026-10-23 (Greg): 1, allowed.** "The linking rule for dynamic libs
is fine." The scanner slices that use it (M5 slices 6 and 8) are deferred
until there are scanners to test against.

## Already acted on (2026-10-23)

- **wasmtime:** PR #33 updates wasmtime, wasmtime-wasi and
  wasmtime-wasi-http from 49.0.2 to 49.0.4 for
  [GHSA-mghf-wjfp-p5xm](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-mghf-wjfp-p5xm)
  (critical). On aarch64, a module with a large `br_table` "could then
  jump to arbitrary other code … leading to arbitrary sandbox escapes":
  Apple Silicon Macs running any installed plugin. The core section also
  cited a second advisory fixed in the same releases,
  [GHSA-5qpp-g3j2-8frx](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-5qpp-g3j2-8frx);
  it couldn't be verified afterwards, so it's unverified here. No backlog
  entry: #33 is the fix. Entry 100.
- **The content interpreter PR** (M11 slice 3): its wrap-up takes
  skrifa/read-fonts for tight glyph boxes (TrueType, CFF and Type 1),
  and hayro-cmap's 65 predefined CMaps, with Adobe's BSD-3 notice. It
  adds backlog F17, hayro-interpret as a second corpus oracle. Entries
  25 and 26 are adopted there.
- **lopdf, for the weekend's upstream pull requests** (Greg,
  2026-10-24/25): 0.46.0 (2026-10-21) has #598 and #603. #624, #630,
  #613 and #625 are merged since. Diff the vendored patch against
  upstream main before writing each PR. Entry 2 has the table, and the
  one discrepancy about #613.

## What the build already compiles

Adopting these adds no new crate, or only a new version. From the
sections' readings of `Cargo.lock` and `cargo tree`.
- **Fonts and text:**
  - ttf-parser (every sheaf-app build, through gpui-pre and usvg);
  - skrifa 0.40 and 0.44, read-fonts 0.37 and 0.41 (the Linux app,
    through cosmic-text and swash);
  - fontdb 0.23 (every platform: gpui-pre turns on resvg's `text` and
    `system-fonts`);
  - harfrust 0.5.2 and unicode-linebreak (Linux, through cosmic-text);
  - rustybuzz 0.20.1 (every build, through usvg; archived);
  - unicode-bidi (every platform through usvg, and Linux through
    cosmic-text);
  - unicode-normalization 0.1.25 (sheaf-doc), unicode-segmentation
    (sheaf-ui and sheaf-app), icu_normalizer 2.3.0 (through `url` and
    `idna`);
  - icu_segmenter, but only inside the boa wasm guest, not the host;
  - a Zed fork of font-kit (GPUI's Linux build).
- **Geometry:** kurbo 0.13.1 (through usvg 0.46); lyon_geom (through
  gpui-pre's lyon 1.0.19).
- **Crypto and transport:**
  - lopdf's aes 0.9.3, cbc 0.2.1, ecb 0.2.1, md-5 0.11.0, sha2 0.11.0,
    stringprep 0.1.5, rand 0.10 and getrandom 0.4;
  - sheaf-doc's hmac 0.13, sha2 0.11 and zeroize 1.9.1;
  - ring 0.17.14, hyper 1.12, tokio (1.53.2 in the lock) and rustls 0.23,
    through wasmtime-wasi and wasmtime-wasi-http;
  - security-framework 3.7.0 (in the lockfile); aws-lc-rs (in the
    lockfile, not in a normal build);
  - oo7 0.6.0 (gpui-pre-linux), on the older RustCrypto line (aes 0.8.4,
    cbc 0.1.2, md-5 0.10.6, sha2 0.10.9): compile time only;
  - uds_windows 1.2.1; windows 0.62.2 (gpui-pre-windows, accesskit) and
    0.61.3 (sheaf-instance, sheaf-print).
- **Images:** image 0.25.10, zune-jpeg 0.5.15, png 0.18, tiff 0.11.3,
  fax 0.2.7, flate2 1.1.10, weezl, imageproc 0.27, moxcms 0.8.1 (through
  image), libloading 0.8.9 and 0.9.0 (through ort, pdfium-render and
  wgpu). zopfli only as a build-time dependency of tesseract-rs.
- **App and tools:** quick-xml 0.41, schemars 1.2.2, accesskit 0.24.1
  with its adapters and accesskit_consumer 0.38, toml 0.9.12, clap 4.6.7,
  proptest 1.12.0.

## The findings by area

Each entry gives the need, where it's needed, today's plan, the
candidates, the fit, and the recommendation with its confidence. Where
two sections covered the same need, the entry says whose detail stands.

### Cross-cutting: the PDF object layer, writing, and Flate

#### 1. The PDF object layer (F13)

- **Need:** read objects from untrusted files lazily, under caps, with
  byte spans; an owned, mutable object model that keeps reals as f64 and
  dictionary order, so copied objects write back unchanged; decode
  streams.
- **Where:** F13 (`todos/backlog.md:688-760`), F15 (`:775-793`);
  `vendor/lopdf/SHEAF.md`.
- **Today's plan:** "Bring what sheaf uses of lopdf into sheaf's own
  crates and drop the dependency, while still sending upstream any fix
  found along the way" (`backlog.md:692-694`). Still lopdf's: "the object
  model; the object parser and cross-reference reading; object streams;
  stream decompression; encryption…; the eager `Document`…; its writer"
  (`:697-706`). "Deferred (Greg, 2026-10-22): revisit after more
  milestones" (`:756`). No move during a milestone.
- **Candidates:**

  | Crate | Latest | License | Maintenance | Rust or FFI, unsafe | wasm |
  |---|---|---|---|---|---|
  | [lopdf](https://crates.io/crates/lopdf) | 0.46.0, 2026-10-21 | MIT | 56 releases since 2016; 26.0M downloads; about 25 PRs merged 2026-10-12 to 23 (J-F-Liu, yhay81, LuisFigueroaG) | Pure Rust; `#![forbid(unsafe_code)]` (0.46.0 `src/lib.rs:2`) | Builds (a `wasm_js` feature for getrandom; unverified beyond that) |
  | [pdf](https://crates.io/crates/pdf) (pdf-rs) | 0.10.0, 2026-03-02 | MIT | 23 releases; 635k downloads; one active maintainer (s3bk), commits 2026-10-13 | Pure Rust; some `unsafe` (`pdf/src/any.rs`) | unverified |
  | [hayro-syntax](https://crates.io/crates/hayro-syntax) | 0.8.0, 2026-10-04 | Apache-2.0 OR MIT | 11 releases since 2025-06; 5.1M downloads; LaurenzV (of Typst); used by `typst-library`, `hayro-interpret`, `hayro-write` | Pure Rust; no `unsafe` unless its `unsafe` feature is on, besides smallvec's | `no_std` with alloc, so it should build (unverified) |
  | [pdf_oxide](https://crates.io/crates/pdf_oxide) | 0.3.78, 2026-09-08 | MIT OR Apache-2.0 on crates.io; README says "MIT licensed"; GitHub detects Apache-2.0 | 83 releases in 11 months; 1.35M downloads; 1,367 commits from one author | Pure Rust core, optional aws-lc crypto; about 96 dependency lines | Has a wasm package |
  | [oxidize-pdf](https://crates.io/crates/oxidize-pdf) | 5.5.1, 2026-10-23 | MIT now; 16 releases were GPL-3.0 and 20 AGPL-3.0 | 119 releases in 15 months, majors 1 to 5 | Pure Rust | unverified |
  | [qpdf](https://crates.io/crates/qpdf) (qpdf-rs) | 0.3.7, 2026-09-27; `vendored` builds qpdf 12.4.2 | Bindings MIT/Apache-2.0; [qpdf](https://github.com/qpdf/qpdf) Apache-2.0 | ancwrd1 alone; 58k downloads. qpdf: 5.5k stars, 12.4.2 on 2026-09-27 | FFI to C++ through `qpdf-c.h`; unsafe throughout | No; F12 plans a separate wasm build |
  | zpdf-parser, pdfpurr | 0.16.0 (2026-10-16), 0.4.0 (2026-03-20) | MIT; MIT OR Apache-2.0 | First published 2026-06 and 2026-03; under 3k downloads | Pure Rust | unverified |

- **Fit, part by part:**
  - **Object model.** None of these is an owned model sheaf could edit.
    - lopdf 0.46.0 still has `Object::Real(f32)` (`src/object.rs:83`).
      sheaf's f64 patch stays: SHEAF.md counts 55,861 reals in 578
      files that a round trip through f32 changed.
    - pdf-rs has `Primitive::Number(f32)` (`pdf/src/primitive.rs:21`);
      its README: "Modifying and writing PDFs is still experimental".
    - pdf_oxide keeps a dictionary as `HashMap<String, Object>`
      (`src/object.rs:23`), so key order is lost.
    - hayro-syntax's objects are borrowed (`Object<'a>`) and read-only:
      "you cannot directly use it to manipulate PDF files" (README).
    - In-housing means lopdf's own `Object`, `Dictionary` and `Stream`
      with the `sheaf:` changes. That keeps the API at the 446 lines in
      182 files that name `lopdf::` today (F13 says "about 400 import
      sites").
  - **Parser, cross-references and repair.**
    - hayro-syntax reads every xref form, xref streams, object streams,
      repairs broken tables, decodes every filter, and decrypts with a
      password (`Pdf::new_with_password` in 0.8.0; its README still says
      password-protected files aren't supported, which the code
      contradicts).
    - Its gaps: it needs the whole file as one slice (`PdfData` is an
      `Arc<dyn AsRef<[u8]>>`, so a memory map works, but there are no
      windowed reads like `FileBytes`). Caps like `ReadLimits` and
      per-object byte spans are unverified. Its crypto is hand-rolled
      (`crypto/aes.rs`, `md5.rs`, `rc4.rs`, `sha256.rs`…), and F13 keeps
      encryption on RustCrypto.
    - qpdf's repair is the best of the lot, but linking it brings C++ to
      three OSes, unsafe FFI, no wasm and qpdf's own object handles.
      sheaf already runs it as a CLI oracle.
  - **Object streams (reading):** small. `vendor/lopdf/src/object_stream.rs`
    is 339 lines, and `LazyPdf` already decodes each one once.
  - **Stream decompression:** keep [flate2](https://crates.io/crates/flate2)
    and weezl (LZW), which lopdf uses already; entry 4 names the Flate
    backend. ASCII85, ASCIIHex, RunLength and the PNG predictor are
    small. Image codecs are entry 33. (Core's note that "the image codecs
    are PDFium's" is superseded there.)
  - **Encryption:** entry 29.
  - **The eager `Document`** (the Rewrite graph, PDFs built in memory,
    `prune_objects`): F15's streaming Rewrite. No crate does a streaming
    copy with pruning over a lazy source.
  - **lopdf's writer** (`annot::tape_pdf`, tests, fixtures): move these
    to `serialize/`. Nothing to adopt.
- **F13's "Maybe published":** no published crate offers an owned, f64,
  order-keeping, lazy, capped object layer with byte spans, so a crate
  of that layer would have a real niche. The nearest are hayro-syntax
  (read-only) and lopdf (eager, f32).
- **Recommendation:** **build in-house** (F13), starting from lopdf's
  MIT code (its object model and reader, with sheaf's patch), on flate2
  and weezl. High.
  - hayro-syntax is the design model for zero-copy lazy parsing.
  - Consider it as a dev-only differential oracle beside
    `source::lazy_tests` (F23): Greg's "hayro as a test oracle", applied
    to the syntax crate. Medium that it's worth the upkeep: its repair
    differs from PDFium's, so every difference needs triage.
  - qpdf (`--check`, `--json`) stays the outside oracle; PDFium's
    `cpdf_parser.cpp` stays the reference for repair.
  - pdf-rs: no. Besides the f32 reals, its README rejects pull requests
    written with AI ("code submitted to this repository needs to be
    written by a human"), so sheaf couldn't send fixes upstream.
  - Every in-housed reader piece gets its fuzz body before it lands, as
    `source::fuzz` does now (entry 99).

#### 2. lopdf fixes upstream (the weekend's pull requests)

- **Need:** know which of sheaf's fixes still need to go upstream.
- **Where:** F13 "Upstream meanwhile" (`backlog.md:726-736`);
  `vendor/lopdf/SHEAF.md:316-324`.
- **Today's plan:** Greg sends them on the weekend of 2026-10-24/25,
  each confirmed first (`backlog.md:726-727`).
- **What upstream has now** (`J-F-Liu/lopdf` `main` at `d0b3abf`,
  2026-10-23; read in the code, not run):

  | sheaf fix (`backlog.md:729-736`) | Upstream |
  |---|---|
  | Strings in stream dictionaries (#603) | **In 0.46.0:** `2f497f8`, "encrypt and decrypt strings in stream dictionaries, and leave signature /Contents in clear (#603, #608)" |
  | V4+ filter names and `/Identity` | **Merged 2026-10-22** (#624, "Fix Identity defaults for V4 and V5 encryption filters"; `encryption.rs:713-715` on main). Whether an undeclared name is now an error: unverified |
  | The set in `traverse_objects` and `prune_objects` | **Merged 2026-10-23** (#630, "traverse and prune in linear time"). Whether it's the same fix: unverified |
  | Rebuilt file with no `trailer` keyword (SHEAF.md) | #613, "reconstruct files that only have cross-reference streams" (see below) |
  | Caps in object streams and predictors | **Merged 2026-10-22** (#625) |
  | `/EFF` | Not upstream (only a doc comment, `encryption.rs:189`) |
  | `/Encrypt null` | Not found upstream |
  | White space around `startxref` | Not upstream: `xref_start` still takes an optional single space before the end of line (`parser/mod.rs:717-726`) |
  | Each xref stream read once; lenient `/XRefStm` past EOF | unverified |
  | f64 reals, `LazyPdf` | Not upstream (`Real(f32)` in 0.46.0) |

  - **#613's timing:** the core section's table and change list put it
    in 0.46.0; its summary lists it among the merges since. Settle it
    when diffing.
  - **#584** (the owner password giving the wrong key) is fixed in the
    vendored code and in 0.46.0 (entry 29), though the issue still shows
    open.
- **Fit:** about half the list was done by others in the last 10 days;
  they're fixing the same corpus-style bugs. sheaf's local branch
  `fix/xref-correction-every-entry` (in `/home/greg/repos/lopdf`) is in
  flight too.
- **Recommendation:** **build on upstream.** Before writing each PR,
  compare the `sheaf:` hunks with upstream `main`, and send only what's
  missing: `/EFF`, `/Encrypt null`, `startxref`, and the
  xref-stream-once fix if it's still needed. Moving the vendored copy to
  0.46+ waits for between-milestone triage, as Greg ruled, and should
  drop the hunks upstream now has. High.

#### 3. Writing PDF: sheaf's serializer against pdf-writer and krilla

- **Need:** write built and copied objects, as an Append or a full file,
  in the base's xref kind, encrypted object by object, byte-faithfully.
- **Where:** M10.5 slices 7, 9 and 10 (built); M14 Optimize and PDF/A;
  M17 tagging.
- **Today's plan:** "sheaf's own (from incremental.rs's writer), with
  append and full framings, the base's xref kind, a real `/Prev`,
  per-object encryption and fsync … Rejected: lopdf's
  `IncrementalDocument`" (`todos/m10.5-decisions.md:1257-1262`).
- **Candidates:**
  - [pdf-writer](https://crates.io/crates/pdf-writer) 0.15.0
    (2026-05-27; MIT OR Apache-2.0; Typst's; 6.2M downloads;
    `forbid(unsafe)`; builds for wasm). Typed step-by-step writers; xref
    streams (`finish_with_xref_stream`). Its numbers are f32
    (`buf.rs:51` `push_float(f32)`), and no object-stream builder was
    found.
  - [krilla](https://crates.io/crates/krilla) 0.8.2 (2026-06-04; MIT OR
    Apache-2.0; LaurenzV; 3.1M downloads; `typst-pdf` 0.15.1 builds on
    it). New documents only, with tagging, PDF/A-1 to 4 and PDF/UA-1.
    Encryption and signatures are "out-of-scope" (README).
  - `hayro-write` 0.8.0: "an internal crate and not meant for external
    use" (README). It turns existing pages into XObjects for pdf-writer.
- **Fit:**
  - Neither crate can write a copied object unchanged (f32), append to
    a file, or encrypt.
  - The new files sheaf makes (B8's comment summary, `tape_pdf`, scans to
    PDF, the print PDF) already go through sheaf's Session and emitter
    (M10.5 slice 19). A second writer would break the one-serializer
    rule.
  - **Worth taking as references:**
    - krilla's `crates/krilla/src/configure/PDF_A1.md` to `PDF_A4.md`
      and `PDF_UA1.md`, which list each standard clause by clause, each
      marked enforced, documented, not applicable or not enforced. With
      them, `validate.rs` (47 KB), which enforces the rules as it
      writes, and its CI checks against veraPDF and the Arlington model
      (README, "Testing");
    - pdf-writer's `structure.rs` (81 KB of typed StructElem, role-map
      and namespace writers) and `attributes.rs`, for key names in M14
      and M17.
- **Recommendation:** **build in-house** (settled): keep `serialize/`,
  adopt neither. Model M14's PDF/A and M17's PDF/UA rules on krilla's
  clause tables. High.

#### 4. Flate: one backend in every build

The images section's detail; core agrees ("keep flate2 (zlib-rs)").
- **Need:** one compressor, so the same input gives the same bytes in
  every build.
- **Where:** every save (lopdf's `Stream::compress` at level 9,
  `vendor/lopdf/src/object.rs:814`); emit (`materialize/emit/mod.rs:860`);
  memory sources (`sheaf-doc/src/images.rs:138`), which rely on "the same
  inputs make the same bytes, so the same blob" (`sheaf-doc/src/memory.rs:5-6`).
  B26.
- **Today:** set by accident.
  - Every sheaf crate asks for `flate2 = "1"` with default features,
    which means miniz_oxide (`crates/sheaf-doc/Cargo.toml:34`,
    `crates/sheaf-ops/Cargo.toml:35`, lopdf).
  - sheaf-app's `plugins` feature brings `zip` with
    `deflate-flate2-zlib-rs` (`crates/sheaf-app/Cargo.toml:19,38`).
    Cargo unifies that into `flate2/zlib-rs`, and flate2 then picks
    zlib-rs (`flate2-1.1.10/src/ffi/mod.rs:69-77`). tesseract-rs's `zip`
    is a build dependency, so it doesn't count.
  - So: the shipped `sheaf` binary (which bundles the CLI) uses zlib-rs;
    the `sheaf-cli` binary, `cargo test -p sheaf-doc` or `-p sheaf-ops`,
    and `--no-default-features` app builds use miniz_oxide;
    `cargo test --workspace` uses zlib-rs.
- **Measured** (flate2 1.1.10, a synthetic 2550×330 gray scan):

  | Level | miniz_oxide | zlib-rs |
  |---|---|---|
  | 1 | 435,764 bytes | 643,987 bytes |
  | 6 | 411,964 bytes | 406,370 bytes |
  | 9 | 411,964 bytes | 411,261 bytes |

  The bytes differ at every level.
- **Fit:** the shipped app is consistent with itself. But a test that
  pins compressed bytes or sizes passes or fails depending on how it's
  run, a dev build without plugins writes different files, and neither
  backend promises the same output across its own versions.
- **Candidates:** [zlib-rs](https://crates.io/crates/zlib-rs) 0.6.8
  (2026-09-15; Zlib license; Trifecta Tech Foundation; about 456 `unsafe`
  in `src/`; 197M downloads); [miniz_oxide](https://crates.io/crates/miniz_oxide)
  0.9.1 (2026-03-13; MIT OR Zlib OR Apache-2.0).
- **Recommendation:** **adopt zlib-rs explicitly** in one workspace entry
  (`flate2 = { version = "1", default-features = false, features =
  ["zlib-rs"] }`), used by every crate. Check whether anything replays a
  memory source across builds or versions and expects the same BlobId.
  High on naming one backend; medium on which (zlib-rs is what ships;
  its own benchmarks say it's faster, unchecked). D4.

### M5: scanner intake (the `m5-intake` branch)

#### 5. SANE on Linux

- **Need:** list SANE scanners, read their options and scan from a
  worker process, with sheaf starting even when libsane is absent.
- **Where:** M5 scanner intake (`ROADMAP.md:130`); `m5-intake.md`
  proposal 6.
- **Today's plan:** build. "`libsane.so.1` is `dlopen`ed by the worker
  (`libloading`)… The bindings are sheaf's: the 15 functions and their
  structs, from SANE's public-domain `sane.h`. The one crate (`sane-scan`
  0.1.2, 2023) links libsane at build time" (`m5-intake.md:615-624`).
- **Candidates:**
  - [`sane-scan`](https://crates.io/crates/sane-scan) 0.1.2 (2023-03-10;
    three releases; MIT on crates.io, though GitHub shows no license
    file; about 4.3k downloads, 22 in the last 90 days; last push
    2024-05). Links libsane at build time and runs bindgen on the system
    headers. Not for wasm.
  - [`libsane-sys`](https://crates.io/crates/libsane-sys) **doesn't
    exist** (crates.io returns 404), and nor does `sane-sys`. The crate
    named [`sane`](https://crates.io/crates/sane) (0.2.1, 2019) is a
    serialization format.
  - About a dozen apps on GitHub carry their own SANE FFI (a code search
    for `sane_get_devices` in Rust); none is a maintained library:
    [libsane-rs](https://github.com/ColinTimBarndt/libsane-rs)
    (Apache-2.0, two days of commits in 2024-06, unpublished) and
    [ruscan](https://github.com/iruizr7/ruscan) (MIT, a GTK4 app, 0.1.0,
    1 star).
  - [`libloading`](https://crates.io/crates/libloading) 0.9.0
    (2025-11-05; ISC; 590M downloads), already in the lockfile.
- **Fit:** sheaf needs run-time loading, cancel from a watchdog thread,
  and its own handling of backend quirks; no crate offers these.
  - Flatpak is a cost the plan defers: it would bundle sane-backends, as
    Simple Scan's does (`m5-intake.md:1090-1095`). That's GPL-2.0+ with
    the SANE exception. Loading it at run time keeps sheaf's license
    intact, but the bundle must offer the backends' source; Flathub's
    public manifests normally cover that (unverified for sheaf). See D17.
- **Recommendation:** **build in-house**, as planned. High.

#### 6. TWAIN on Windows, with a 32-bit worker

- **Need:** drive TWAIN data sources, 64- and 32-bit. The office's
  Fujitsu fi-6130s use the 32-bit FUJITSU TWAIN32 driver.
- **Where:** M5 (`m5-intake.md` proposal 7, asks 3 and 12).
- **Today's plan:** build. "Bindings: sheaf's own, from `twain.h` (the
  `twain` crate is a single 2021 release that ships DLLs inside it).
  RSWebTWAIN … is the reference" (`m5-intake.md:675-678`).
- **Candidates:**
  - [`twain`](https://crates.io/crates/twain) 0.1.0 (2021-07-24, the only
    release; MIT; about 2k downloads; 791 KB because it carries DLLs;
    FFI). No `twain-sys` exists.
  - [RSWebTWAIN](https://github.com/seancampbell3161/RSWebTWAIN) (MIT and
    Apache-2.0 files; 0 stars; pushed 2026-10-16): a Tauri app, "alpha",
    not a crate. Its typestate TWAIN state machine and 32-bit JSON-lines
    sidecar are the plan's reference.
  - Other Rust TWAIN FFI, such as `jtubbenhauer/twain-rs` (MIT, one day
    of commits in 2024), is app-internal or abandoned.
  - The TWAIN DSM (`TWAINDSM.dll`, LGPL-2.1): the plan ships it
    unmodified and loads it at run time (`m5-intake.md:660-662`). That's
    LGPL-compatible, but a cost: the license text, a source offer or
    link, and four more PE files to sign (the plan lists the first and
    the last). See D17.
- **Fit:** no crate covers 32- plus 64-bit, the hidden window and
  message loop, or memory transfer.
- **Recommendation:** **build in-house**, as planned. High.

#### 7. WIA 2.0 on Windows

- **Need:** feeder and duplex scanning, and network copiers through
  Windows' WSD and eSCL drivers.
- **Where:** M5 (`m5-intake.md` proposal 8).
- **Today's plan:** adopt `windows`: "`Win32::Devices::ImageAcquisition`:
  `IWiaDevMgr2`, `IWiaItem2`, `IWiaTransfer`, `IWiaTransferCallback`"
  (`m5-intake.md:680-683`).
- **Candidates:** [`windows`](https://crates.io/crates/windows) 0.62.2
  (2025-10-06; MIT OR Apache-2.0; Microsoft; 375M downloads). Still the
  latest: windows-rs's 2026 releases published only windows-bindgen
  0.66, windows-metadata 0.60 and windows-rdl
  ([releases](https://github.com/microsoft/windows-rs/releases)). Feature
  `Win32_Devices_ImageAcquisition` has `IWiaDevMgr2`, `IWiaItem2`,
  `IWiaTransfer`, `IWiaTransferCallback`, `WIA_IPS_DOCUMENT_HANDLING_SELECT`,
  `WIA_IPS_PAGES` and `WIA_ERROR_PAPER_EMPTY`
  ([docs](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Devices/ImageAcquisition/index.html)).
- **Fit:** complete for the calls planned. The undocumented `0x210001`
  status is a raw HRESULT sheaf matches itself.
- **Recommendation:** **adopt.** Put the worker on GPUI's version (0.62)
  so the 64-bit build adds nothing. High.

#### 8. macOS ImageCaptureCore (not in M5)

- **Need:** scanning on macOS, later.
- **Where:** `m5-intake.md` ask 4 (`:1217-1229`), a Pending entry.
- **Today's plan:** "`objc2-image-capture-core` (0.3.2, which has
  `ICScannerDevice` and the feeder unit)".
- **Candidates:** [`objc2-image-capture-core`](https://crates.io/crates/objc2-image-capture-core)
  0.3.2 (2025-10-04; Zlib OR Apache-2.0 OR MIT; madsmtm's objc2, pushed
  2026-10-17). Still current: every objc2 framework crate is on 0.3.2.
  Features `ICScannerDevice`, `ICScannerFunctionalUnits` and
  `ICScannerBandData`. FFI to a system framework, which passes the
  plan's "only system libraries" gate.
- **Recommendation:** **adopt**, when it's scheduled. Medium (no Mac
  scanner to test with).

#### 9. eSCL (not planned)

- The plan's "no network scan protocol of sheaf's own" stands
  (`m5-intake.md:600-607`).
- If it's ever wanted: [`escl`](https://crates.io/crates/escl) 0.3.0
  (2024-09-18; MIT OR Apache-2.0; about 5k downloads) and
  [`mdns-sd`](https://crates.io/crates/mdns-sd) 0.21.6 (2026-10-19;
  Apache-2.0 OR MIT; 6.6M downloads; frequent releases).
- No change.

### M5: separator sheets

#### 10. Barcode reading

- **Need:** read QR, Data Matrix, Code 39, Code 128 and PDF417 on page
  images at 150–200 dpi in 1-bit, with no false reads on real scans.
- **Where:** M5 separators (`ROADMAP.md:131`; `m5-intake.md` proposals
  14–15, ask 7); F16 (QR codes into links, a wasm plugin).
- **Today's plan:** "**rxing, behind a small trait in
  `sheaf_image::codes`**… the trait lets zxing-cpp in later"
  (`m5-intake.md:1243-1247`).
- **Candidates:**
  - [`rxing`](https://crates.io/crates/rxing) 0.9.3 (2026-09-16, the
    latest; four releases in 2026; Apache-2.0; hschimke alone; 736k
    downloads). Pure Rust, no `unsafe` in `src/`.
    [rxing-wasm](https://github.com/rxing-core/rxing-wasm) targets
    wasm32-unknown-unknown; wasm32-wasip2, which F16 needs, is
    unverified. Its `image` feature pins `image = "=0.25.8"` and pulls
    `imageproc ^0.25` (sheaf is on 0.27); `default-features = false`
    drops both, as the plan says.
  - [`zxing-cpp`](https://crates.io/crates/zxing-cpp) 0.5.3 (2026-07-29;
    Apache-2.0; 33k downloads): builds C++20 with CMake. FFI; not for
    wasip2.
  - [`rqrr`](https://crates.io/crates/rqrr) 0.11.0 (2026-09-02; (MIT OR
    Apache-2.0) AND ISC): QR only.
  - [`zbar-rust`](https://crates.io/crates/zbar-rust) 0.1.1 (LGPL-2.1)
    and [`zedbar`](https://crates.io/crates/zedbar) 0.7.0 (2026-10-02,
    LGPL-3.0-or-later): copyleft, so prior art only.
  - [`bardecoder`](https://crates.io/crates/bardecoder) 0.5.0 (2023): QR
    only, stale.
- **Fit:** as the plan's benchmark found; nothing has changed. For F16,
  read QR only: reading every format finds false codes. F16's host
  `barcodes` interface beside `ocr` (`backlog.md:978-980`) would let
  plugins reuse M5's reader.
- **Recommendation:** **adopt behind a trait** (rxing first, zxing-cpp
  second), as planned. High.

#### 11. Barcode writing (sheaf's QR sheet)

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

#### 12. Kodak patch codes

- **Need:** find patch bars (types 1, 2, 3, 4, 6, T) along a page's
  edges.
- **Where:** `m5-intake.md` proposal 14 (`:868-876`).
- **Today's plan:** build. "sheaf reads them itself (no barcode library
  does…)".
- **Candidates:** none. crates.io ("patch code", "patchcode", "kodak
  patch") and GitHub have no detector, and zxing-cpp has none.
- **Recommendation:** **build in-house**, as planned. The prototype read
  204/204 synthetic pages and 60/60 renders of Kodak's sheets. High.

### M5 (done): OCR, scan cleanup, image import

#### 13. OCR engines

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

#### 14. Scan cleanup

- **Need:** deskew, despeckle, Sauvola binarization, page finding,
  lighting.
- **Where:** M5, done (`ROADMAP.md:116-119`).
- **Today:** `sheaf_image::clean` uses imageproc 0.27 for blur, Canny,
  morphology, contours, warps, median filters and connected components.
  sheaf's own: the projection-profile deskew (`clean.rs:490`), Sauvola
  on integral images (`:675`), despeckle over imageproc's labelling
  (`:815`).
- **Candidates:**
  - [`imageproc`](https://crates.io/crates/imageproc) 0.27.0
    (2026-06-02; MIT; image-rs): `adaptive_threshold`, Otsu, Kapur and
    Hough, but **no Sauvola and no deskew** (its 0.27.0 `contrast.rs` and
    `hough.rs`).
  - Searches for "sauvola", "deskew" and "binarization" found only small
    apps and one-off crates.
  - Leptonica has all of these. sheaf links it statically inside
    Tesseract (BSD-2), but tesseract-rs exposes no Leptonica API;
    [`leptonica-sys`](https://crates.io/crates/leptonica-sys) 0.4.9
    (2024) is a separate C link.
- **Recommendation:** **build in-house** (as done): nothing better
  exists, and sheaf's code is a few hundred lines. High.

#### 15. Image import, and where sheaf codes images today

- **Where:** M5 images-to-pdf, M7 export, M8 inserts (all done); M5
  intake slice 1 (G4 for black and white, JPEG for color and gray).
- **PDFium decodes every PDF image filter, but only to render.** sheaf
  never asks it for image data (no `PdfPageImageObject` use in
  `crates/`). PDFium's own codecs: libjpeg-turbo, an OpenJPEG fork, its
  own JBIG2 and CCITT decoders, zlib, and lcms2 for color.
- **sheaf decodes itself:**
  - image files in `sheaf_image::io`: JPEG through image (zune-jpeg
    0.5.15), plus sheaf's own SOF and EXIF parse for pass-through; PNG
    through png 0.18; TIFF through tiff 0.11.3, with `fax` for G4 and
    zune-jpeg for JPEG-in-TIFF; BMP, GIF and WebP through image;
  - PDF stream filters through lopdf: Flate, LZW, A85, AHx, RL and
    Brotli, with predictors and size caps, on flate2 and weezl;
  - inline images' ends, in the content lexer (`content/lex.rs:549-563`).
- **sheaf encodes itself:** PNG with png (`sheaf-image/src/encode.rs:14`);
  JPEG with image's encoder (`:29-44`); LZW TIFF with tiff (`:69`), with
  G4 TIFF directories sheaf writes; **CCITT G4 with `fax` 0.2's
  `Encoder`** (`:128-143`, B24); Flate with flate2
  (`sheaf-doc/src/images.rs:137-142`, `materialize/emit/mod.rs:860`,
  lopdf's `Stream::compress` at level 9).
- **Gap found:** tiff 0.11.3 refuses fax TIFFs other than G4 (entry 16,
  B25).
- **Recommendation:** keep this stack; fix B24 and B25.

#### 16. TIFF

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

### M10: Forms

#### 17. The Noto fallback: a glyph-ID-preserving Type0 subset, late-bound per save

- **Need:** embed only the Noto Sans glyphs an output draws, as Type0
  with `CIDToGIDMap /Identity`, `/W` and a per-glyph ToUnicode. An
  Append adds a new subset object when new glyphs appear.
- **Where:** M10 slice 7 (`m10.md:1454-1477`); M10.5 §13. D1.
- **Today's plan:**
  - "a GID-preserving Noto Type0 subset (the rule the `Font` trait's
    docs set for it), late-bound per materialization or overlay"
    (`m10.md:1455-1460`).
  - `content/font/mod.rs:35-38`: "Any font sheaf embeds is
    GID-preserving: a subset keeps each glyph's number from the whole
    font (CID = GID, `/CIDToGIDMap /Identity`), so the codes a string is
    drawn with depend only on the font and the text, and re-subsetting
    never changes them."
  - `m10.md:633` names "`subsetter`"; Greg leans to it.
  - M10.5 slice 2's `gen.py` already has a stdlib TrueType subsetter
    that "keeps glyph ids, empties the glyphs a section doesn't use
    (following composites)" (`m10.5-edit-api.md:6889-6899`).
- **Candidates:**
  - [subsetter](https://crates.io/crates/subsetter) 0.2.6 (2026-06-04;
    MIT OR Apache-2.0; 8.1M downloads; 3 releases in 12 months; Typst's,
    published by laurmaedje; used by krilla and so by typst's PDF export;
    `deny(unsafe_code)`; [GitHub](https://github.com/typst/subsetter)).
    The default `variable-fonts` feature pulls skrifa **0.42**,
    write-fonts 0.48 and kurbo; without it, only `rustc-hash`. **It
    always renumbers:** "glyph IDs in fonts must be consecutive. So if we
    only include the glyphs 68, 69, 70 … in the new font they will have
    the glyph IDs 1, 2, 3" (lib.rs docs); `GlyphRemapper` assigns IDs in
    insertion order. Input must be SFNT (`glyf` or `CFF `); CFF2 only
    with that feature; no bare CFF, no Type 1.
  - [skera](https://crates.io/crates/skera) 0.9.0 (2026-10-21; MIT OR
    Apache-2.0): fontations' Rust port of hb-subset. Young: first release
    2026-02-10, 14 releases, 35.9k downloads; used by fontations'
    incremental-font-transfer. Has `SUBSET_FLAGS_RETAIN_GIDS`. Subsets
    `glyf`/`loca`, `hmtx`, `cmap`, GSUB/GPOS, `gvar` and more; **no CFF
    table subsetter** in 0.9.0's `subset_table`. No `unsafe` in its
    source (rg), but no `forbid`. Depends on read-fonts 0.46, skrifa
    0.49, write-fonts 0.55 and hashbrown.
  - HarfBuzz hb-subset (has retain-gids): [hb-subset](https://crates.io/crates/hb-subset)
    0.3.0 (MIT; 2023-11-12; 12.8k downloads; stale; bundles HarfBuzz C++).
    [harfbuzz-sys](https://docs.rs/harfbuzz-sys/0.8.0/harfbuzz_sys/all.html)
    0.8.0 has `hb_shape*` but no `hb_subset_*`.
  - allsorts 0.17.0: `subset(provider, glyph_ids, …)` numbers the output
    by the order of `glyph_ids`, so it renumbers too. Apache-2.0.
  - "klippa" on crates.io is an [unrelated crate](https://crates.io/crates/klippa);
    fontations' subsetter is skera.
- **Fit:**
  - The keep-glyph-IDs rule is worth keeping: the emitter writes codes
    before it knows the subset, appearance digests stay independent of
    the save, and an Append's earlier subset stays valid.
  - subsetter breaks it: adopting it means codes chosen per output (two
    passes in the emitter), and dropping the rule from M10.5's `Font`
    docs.
  - skera fits Noto (a `glyf` font) with RETAIN_GIDS, but it's
    general-purpose: its output includes cmap and layout tables unless
    they're dropped.
  - The input is sheaf's own bundled font, not untrusted. A
    purpose-built subset is small: keep `glyf`/`loca` for the used glyphs
    and their composite closure; keep `hmtx`, `head`, `hhea` and `maxp`;
    `post` format 3; drop cmap, layout and hinting. It's deterministic
    across crate bumps, which matters because `FontRes` ids are digests
    of the bytes.
  - Measured: Noto Sans Regular, unhinted, is 431,364 bytes with 3,884
    glyphs. Keeping every glyph ID costs `loca` and `hmtx` about 15.5 KB
    each before Flate, mostly repeated entries (the compressed size is
    unverified).
- **Recommendation:** **build in-house on read-fonts** (a port of
  gen.py's algorithm), with **skera + `RETAIN_GIDS`** as the alternative
  if it grows. Not subsetter while the rule stands. Medium. If Greg would
  rather drop the rule, subsetter is the mature choice.

#### 18. Appearance layout: comb cells, auto size, wrapping

- **Need:** lay out a field value as Acrobat does: `/Q` alignment, `0 Tf`
  auto size, multiline wrapping by real widths, comb cells from
  `/MaxLen`, `DoNotScroll`. The field editor uses the same layout.
- **Where:** M10 slice 6 (`m10.md:1432-1435`); slice 11.
- **Today's plan:** build ("Layout: `/Q`; auto size (`0 Tf` …); multiline
  wrapping by real widths; comb cells from `/MaxLen` …").
  `content/font/mod.rs:68-101`'s `wrap_with` splits paragraphs on `\n`
  and words on spaces.
- **Candidates:** [textwrap](https://crates.io/crates/textwrap) 0.16.4
  (MIT, 2026-09-13; takes a custom width function); unicode-linebreak and
  icu_segmenter (entry 60).
- **Fit:** the rules are Acrobat's, not a typesetter's; comb cells and
  auto size are PDF-specific. `wrap_with` already measures through the
  `Font` trait across mixed fonts (the field font plus the fallback).
  textwrap adds nothing for Latin values.
- **Recommendation:** **build in-house** (as planned). High. Add UAX #14
  break opportunities only when M13 needs them.

#### 19. Unicode basics: NFC, grapheme clusters, the right-to-left notice

- **Need:** NFC at commit; `MaxLen` and comb cells by grapheme cluster;
  detecting right-to-left text for the notice.
- **Where:** M10 slice 7's lowering (`m10.md:1461-1464`); proposal 11
  (`:612-618`).
- **Today's plan:** "Values are normalized to NFC at commit … `MaxLen` and
  comb cells count grapheme clusters … Right-to-left text and complex
  shaping are out, with a notice" (`m10.md:612-615`). The dependencies
  are to be named (`:632-635`).
- **Candidates:**
  - [unicode-normalization](https://crates.io/crates/unicode-normalization)
    0.1.26 (2026-10-16): sheaf-doc's dependency already (locked at
    0.1.25). From 0.1.26 its license is "(MIT OR Apache-2.0) AND
    Unicode-3.0", for its data tables: permissive, but a notice to ship
    (F18). 634M downloads; `no_std`.
  - [icu_normalizer](https://crates.io/crates/icu_normalizer) 2.4.0
    (2026-10-22; Unicode-3.0; ICU4X): in the lock already (2.3.0, through
    `url` → `idna`). No reason to switch.
  - [unicode-segmentation](https://crates.io/crates/unicode-segmentation)
    1.13.3 (2026-06-01; MIT OR Apache-2.0; 629M downloads): sheaf-ui and
    sheaf-app use it (workspace dependency, `Cargo.toml:72`); sheaf-doc
    or sheaf-edit would add it.
  - [unicode-bidi](https://crates.io/crates/unicode-bidi) 0.3.18
    (2024-12-16; MIT OR Apache-2.0; servo; 550M downloads; Unicode 16.0
    tables; one `unsafe`): `bidi_class(c)` gives R/AL for the notice.
- **Fit:** complete for these uses, pure Rust and wasm-friendly.
- **Recommendation:** **adopt** unicode-normalization (keep),
  unicode-segmentation (in sheaf-doc too) and unicode-bidi. High.

#### 20. The field editor's painting (slice 11)

- **Need:** paint the value being typed so it lines up with the
  appearance it saves as: Liberation for what the field's font encodes,
  Noto for the fallback runs, and the embedded font when there's no
  Liberation match.
- **Where:** M10 slice 11 (`m10.md:1584-1590`); proposal 11
  (`:619-631`). D9.
- **Today's plan:** "A field whose embedded font has no Liberation match
  (OCRAStd, 68 fields) is painted in that embedded font if GPUI can load
  it, else in Liberation Mono for a fixed-pitch font and Liberation Sans
  otherwise" (`m10.md:624-627`).
- **Candidates:** GPUI itself:
  - `TextSystem::add_fonts(Vec<Cow<[u8]>>)` (`gpui-pre-0.3.9/src/text_system.rs:295`);
  - `Window::paint_glyph(origin, font_id, glyph_id, size, color)`,
    "only useful if you need to paint a single glyph that has already
    been shaped" (`window.rs:4821`); `GlyphId(pub u32)` can be
    constructed;
  - skrifa outlines, drawn as GPUI paths.
- **Fit:**
  - Shaping through GPUI (CoreText, DirectWrite, cosmic-text) can place
    glyphs differently from sheaf's layout. Painting sheaf's own glyph
    run (glyph IDs from skrifa on the same bundled bytes) with
    `paint_glyph` matches the appearance by construction.
  - **Security:** `add_fonts` with a PDF's embedded program hands
    untrusted bytes to CoreText (macOS) or DirectWrite (Windows), C and
    C++ font stacks with a history of font-parsing CVEs, and registers
    the font for the process's lifetime under the PDF's family name. On
    Linux it goes to fontdb and ttf-parser, which are Rust.
- **Recommendation:** **build on GPUI's `paint_glyph`**, with glyph runs
  from sheaf's layout, for the bundled fonts. Don't `add_fonts` a PDF's
  program: draw its skrifa outlines as paths, or use the Liberation
  fallback. Medium (`paint_glyph` with fonts from `add_fonts` is
  unverified on macOS and Windows).

#### 21. The bundled fonts and their licenses

- **Need:** ship Noto Sans (the embedded fallback) and Liberation Sans,
  Serif and Mono (the editor; later metric-compatible substitutes), with
  correct notices.
- **Where:** M10 slices 7 and 11 (`m10.md:1469`, `:1597`); M10.5's M14
  notes (`m10.5-edit-api.md:3667`).
- **Today's plan:** "Noto Sans, under the SIL OFL (kickoff)"
  (`m10.md:603`); "The Liberation fonts' license in `packaging/licenses`"
  (`:1597`).
- **Facts:**
  - **Noto Sans:** latest NotoSans-v2.015, 2024-11-20
    ([repo](https://github.com/notofonts/latin-greek-cyrillic)). Its
    OFL.txt names no Reserved Font Name. Latin, Greek and Cyrillic only;
    Arabic, Hebrew, Indic and CJK are other families, which matches the
    plan's refusal. Unhinted Regular is 431,364 bytes (hinted 621,572).
  - **Liberation 2.1.5** (2021-09-30;
    [repo](https://github.com/liberationfonts/liberation-fonts)): OFL
    1.1, with Reserved Font Names "Liberation" and Google's "Arimo,
    Tinos and Cousine". 1.x was GPL with a font exception, so take 2.x.
    Last pushed 2022-12, fine for finished fonts.
  - **[OFL-FAQ](https://openfontlicense.org/ofl-faq/):** 1.12, embedding
    in documents is allowed "either in full or a subset", and the
    modification restrictions "do not apply"; 1.20, bundling in an app
    needs the copyright, license notice and license text. So subsetting
    either family into a PDF is fine. Ship Liberation unmodified in the
    app (don't subset or rename the bundled copy), or the RFNs apply.
- **Recommendation:** **adopt** both, unmodified, with `LICENSE-noto`
  and `LICENSE-liberation` in `packaging/licenses` (F18). Ship Noto
  unhinted. Bundle only the faces the editor rule uses. High.

#### 22. CSV and tab-delimited data (slice 17)

- **Need:** many forms into one CSV (UTF-8 with a BOM, columns in tab
  order, an optional `#tooltip` second header row, an Excel-safe prefix
  on formula-like values); tab-delimited text out, and in through a row
  picker.
- **Where:** M10 slice 17 (`m10.md:1714-1725`); proposal 17
  (`:1011-1029`). Later: M15's "a filled copy per CSV row"
  (`:2044-2045`); table export (`docs/prior-art/rust-ecosystem.md:366`).
- **Today's plan:** the formats are specified; no crate named.
- **Candidates:** [csv](https://crates.io/crates/csv) 1.4.0 (2025-10-17;
  Unlicense OR MIT; BurntSushi; 274M downloads; `csv-core` is `no_std`;
  builds for wasm).
- **Fit:** csv quotes fields and switches delimiters correctly, the part
  that's easy to get wrong. The BOM, the `#tooltip` row and the
  `=`/`+`/`-`/`@` apostrophe are sheaf's. Whether the reader strips a BOM
  is unverified; it's trivial either way.
- **Recommendation:** **adopt csv.** High.

#### 23. FDF and XFDF (slice 16)

- **Need:** read and write form values beside comments, which already
  round-trip since M9.
- **Where:** M10 slice 16 (`m10.md:1700-1713`).
- **Today's plan:** sheaf's own `sheaf_doc::exchange`: FDF on the lopdf
  parser, XFDF on quick-xml.
- **Candidates:** [quick-xml](https://crates.io/crates/quick-xml) (sheaf
  locks 0.41; 0.42.0 is out, 2026-08-22; MIT). pdf_oxide has an
  `src/fdf/` module (not reviewed). No crate does FDF and XFDF for an
  existing field model.
- **Fit:** values only, matched by full name, scripts and flag changes
  ignored (proposal 17): sheaf's own policy.
- **Recommendation:** **build in-house** (as planned) on quick-xml; bump
  quick-xml when convenient. High.

#### 24. Form scripting (slices 18–22)

- **Need:** Acrobat's AF functions in Rust (slice 18), and pdf.js's
  `scripting_api` in boa, run as a wasm guest (slices 19–22).
- **Where:** `m10.md:81-92` (decision 1), `:104-…` (decision 3),
  `:1726-1892`; `ROADMAP.md:301-302`.
- **Today's plan:** "The form JavaScript engine is boa_engine"
  (`m10.md:81`); "boa runs as a WebAssembly guest in sheaf's wasmtime"
  (`:104`); "boa supports only its latest 0.x release" (`:265`).
- **What's changed:**
  - [boa_engine](https://crates.io/crates/boa_engine) is still 0.22.0
    (2026-08-28; Unlicense OR MIT). Its `main` raised the MSRV to 1.95 on
    2026-10-17; sheaf is on 1.98.
  - The issues that made boa unsafe in process are still open:
    [#3442](https://github.com/boa-dev/boa/issues/3442) (interrupt),
    [#4397](https://github.com/boa-dev/boa/issues/4397) and
    [#4311](https://github.com/boa-dev/boa/issues/4311) (parser stack
    overflow; updated 2026-10-21). The wasm decision holds.
  - [rquickjs](https://crates.io/crates/rquickjs) 0.14.0 (2026-09-18;
    MIT) binds QuickJS in C: in process it's C parsing hostile JS, and as
    a guest it needs wasi-sdk (as F12's qpdf does). Nothing favors it.
  - The AF port: on rustc 1.99, `format!("{:.2}", 0.125)` gives `0.12`
    where JavaScript's `(0.125).toFixed(2)` gives `0.13` (likewise 2.5
    to 0 places: `2` against `3`). The plan covers it: "The port rounds
    as JavaScript's `toFixed` does" (`m10.md:739-740`).
- **Recommendation:** unchanged. **Adopt boa 0.22 as a wasm guest,**
  rquickjs only if a guest gate fails, and **build the AF functions in
  Rust.** High.

### M11: Security and redaction

#### 25. Font programs: outlines, metrics and boxes

- **Need:** read the programs PDFs embed (`FontFile` Type 1, `FontFile2`
  TrueType, `FontFile3` Type1C / CIDFontType0C / OpenType). The
  interpreter needs advances, glyph boxes, ascent and descent, glyph
  names and the built-in encoding, the char → glyph mapping, and whether
  a glyph exists.
- **Where:** M11 slice 3 (glyph boxes) and slice 5 (blanking, entry 28);
  M10 slices 6–7 (`covers()` for an embedded `/DR` font, the editor
  rule); M13's missing-glyph warning; M14's PDF/A font checks.
- **Today's plan:** m11.md:459-463, slice 3: "Fonts, giving glyph boxes:
  widths from `/Widths` and CID `/W`; heights from `/FontDescriptor`
  (`Ascent`, `Descent`, `FontBBox`); the standard 14 fonts' metrics;
  Type3 `FontMatrix`." Its check: "glyph boxes match PDFium's character
  boxes within a tolerance" (`:471`). `m10.md:632-634` names
  "`ttf-parser`". Greg leans to skrifa. The interpreter in-house on
  `content/`, with hayro-interpret an oracle only (M10.5's D55,
  `m10.5-decisions.md:1472-1491`). **Taken since:** done in the content
  interpreter PR (M11 slice 3, its wrap-up): skrifa 0.44.0 / read-fonts
  0.41.0, default features off (`std` only), the versions GPUI's Linux
  build already compiles through swash, so no third version; they read
  `glyf`, bare CFF (CID-keyed included), OpenType CFF and Type 1. Glyph
  boxes are PDFium's loose boxes, heights follow `CheckFontMetrics`,
  behind sheaf's own `content/font/program.rs` with its work caps
  ([Already acted on](#already-acted-on-2026-10-23)). m10.md decision
  11 and slice 7, and ROADMAP's M13 and M14, name skrifa and subsetter
  (Greg, 2026-10-24).
- **Why the program matters, even for slice 3:**
  - PDFium's `CPDF_Font::CheckFontMetrics` fills an all-zero `FontBBox`,
    and `Ascent`/`Descent`, from the font file. If Ascent and Descent are
    both 0, it estimates them from the boxes of 'A' and 'g'.
    `LoadFontDescriptor` negates a `Descent` above 10
    ([cpdf_font.cpp](https://pdfium.googlesource.com/pdfium/+/HEAD/core/fpdfapi/font/cpdf_font.cpp)).
    Matching PDFium's boxes on such files needs the program.
  - What real files embed (`../pdf-corpus/index/safedocs-parse-inventory.tsv.gz`,
    127,919 general web PDFs, not tax binders):

    | Font type | Files |
    |---|---|
    | Type0 | 54,926 |
    | CIDFontType2 | 49,026 |
    | TrueType | 38,026 |
    | Type1 (unembedded standard 14 included) | 36,835 |
    | CIDFontType0 (CFF) | 5,958 |
    | Type3 | 504 |
    | MMType1 | 21 |

    38,086 have an unembedded font. All three program formats occur.
- **Candidates:**
  - **skrifa 0.49.0 and read-fonts 0.46.0** (both 2026-10-21;
    [skrifa](https://crates.io/crates/skrifa),
    [read-fonts](https://crates.io/crates/read-fonts),
    [fontations](https://github.com/googlefonts/fontations)):
    - MIT OR Apache-2.0. skrifa: 88 versions, 16 releases in 12 months,
      39.0M downloads; read-fonts: 105 versions, 19 releases, 41.5M; the
      repo: 848 stars, 321 open issues, pushed 2026-10-23. Google Fonts
      (published by dfrg); skrifa's goal is to replace FreeType in
      Google's applications.
    - Users: an experimental Fontations backend in Chrome/Skia. PDFium's
      `core/fxge/skrifa` tries SFNT, then `CffFontRef`, then `Type1Font`
      ([main.rs](https://pdfium.googlesource.com/pdfium.git/+/HEAD/core/fxge/skrifa/src/main.rs)).
      Also hayro-interpret 0.8 (skrifa ^0.46), krilla 0.8.2, cosmic-text
      and swash.
    - Coverage (read in the source): `read_fonts::model::Format` =
      `Sfnt | Type1 | Cff` ("A standalone CFF font"); `Font::new(source,
      index)` takes all three; `ps::type1::Type1Font` has `advance`,
      `extents`, `glyph_names`, `encoding`, `unicode_charmap`, `bbox` and
      multiple master; `ps::cff` handles CFF; skrifa draws "CFF or Type 1
      charstrings".
    - Type 1 arrived in 2026: read-fonts 0.37 (2026-01-07) has no `ps`
      module, 0.41 (2026-07-03) has `ps/type1.rs`. hayro switched on
      2026-04-03: "Replace usages of `hayro-font` with skrifa"
      ([hayro #1153](https://github.com/LaurenzV/hayro/pull/1153)).
    - Pure Rust; both `#![forbid(unsafe_code)]`. `no_std`-capable; a wasm
      build is unverified.
    - Weight: 349 KB (skrifa) and 921 KB (read-fonts) packaged. The Linux
      app already compiles two older versions of each, so this adds a
      third there; for sheaf-cli it's new. Breaking 0.x releases come
      about monthly (0.44 → 0.49 in 3.5 months), so each bump is real
      work.
  - **[ttf-parser](https://crates.io/crates/ttf-parser) 0.25.1**
    (2024-11-29; [GitHub](https://github.com/harfbuzz/ttf-parser); MIT OR
    Apache-2.0; `forbid(unsafe_code)`; 124M downloads, but no release in
    12 months, pushed 2026-08-06). TrueType/OpenType, and bare CFF
    through `ttf_parser::cff::Table::parse`. **No Type 1.** Already in
    every sheaf-app build.
  - **[hayro-font](https://crates.io/crates/hayro-font) 0.4.0**
    (2026-01-08; Apache-2.0 OR MIT): a ttf-parser fork for CFF and Type
    1, "an internal crate and not meant to be used directly … not
    well-documented". Its Type 1 table has no advances, and hayro dropped
    it. **Don't use.**
  - **[allsorts](https://crates.io/crates/allsorts) 0.17.0** (2026-05-13;
    Apache-2.0; YesLogic, used by Prince; 1.14M downloads):
    OpenType/WOFF/WOFF2 with its own shaper and subsetter; no Type 1
    parsing found; 39 `unsafe` sites; pulls libc, flate2 and encoding_rs.
    Narrower than skrifa for PDF use.
  - Type 1 elsewhere: `type1-encoding-parser` 0.1.1 (MIT; the encoding
    only). Nothing else maintained.
- **Fit:**
  - skrifa covers every program format sheaf meets, in memory-safe Rust,
    and it's what PDFium is moving toward, which helps "match PDFium".
  - Sheaf's still: the PDF layer (encodings, `/Differences`, symbolic
    TrueType cmap (3,0) quirks, `/CIDToGIDMap`); work caps on untrusted
    programs (skrifa bounds charstring recursion, but sheaf caps program
    size through `source::Limits` and caches one parsed font per object,
    for 7,000-page files); Type3, which isn't a font program.
  - skrifa only reads, so edits to font objects stay byte-preserving.
- **Recommendation:** **adopt skrifa (read-fonts) behind sheaf's own
  font-program module.** High. Slice 3 can ship its box math on
  `/Widths` and the descriptor first, then use skrifa where the
  descriptor lacks metrics, and for M10's `covers()`, M11's blanking,
  M13 and M14. Drop `ttf-parser` from `m10.md:633`.

#### 26. CMaps: predefined CJK CMaps and CID → Unicode

- **Need:** for a Type0 font whose `/Encoding` is a predefined CMap
  (90ms-RKSJ-H, UniJIS-UCS2-H, UniGB-UCS2-H, KSCms-UHC-H…), split strings
  into codes and map code → CID, for `/W` widths and glyph boxes. Map CID
  → Unicode through Adobe-\*-UCS2 when there's no `/ToUnicode`, for text,
  the removal check's `TextBag` and the leak check. M13 needs Unicode →
  CID to write such fonts.
- **Where:** M11 slices 3 and 5; M10.5's `FontDecoders`
  (`crates/sheaf-doc/src/content/text_bag.rs:146-230`); M13.
- **Today's plan:** none. sheaf parses `/ToUnicode` itself
  (`text_bag.rs:322-`: bfchar, bfrange, codespace ranges). A Type0 font
  with no ToUnicode decodes 2 bytes at a time (`:223-229`): its strings
  become hashes, consistent but not text. **Taken since:** done in the
  content interpreter PR (M11 slice 3, its wrap-up): hayro-cmap 0.1.0
  (with hayro-postscript 0.1.0 and brotli 8.0.4) for the 65 bundled
  maps, CIDs for `/W` and CID → Unicode where there's no ToUnicode;
  sheaf's ToUnicode parser kept, PDFium's code splitting kept in
  sheaf's own table while the codespace stays private (an upstream PR
  to expose it is for later: backlog F25). Adobe's notice ships as
  `packaging/licenses/LICENSE-cmap-resources`, brotli's as
  `LICENSE-brotli`.
- **Candidates:**
  - **[hayro-cmap](https://crates.io/crates/hayro-cmap) 0.1.0**
    (2026-04-12; [docs](https://docs.rs/hayro-cmap/0.1.0)):
    - Apache-2.0 OR MIT for the code. The bundled data is under Adobe's
      BSD-3 notice, "Copyright 1990-2023 Adobe" (`assets/LICENSE.txt`),
      which a binary distribution must reproduce (F18).
    - One release; 3.45M downloads, 3.24M of them in 90 days, riding on
      hayro-interpret. The hayro repo: 814 stars, pushed 2026-10-23.
    - Pure Rust, `forbid(unsafe)`, `no_std` without its default feature.
      Depends on hayro-postscript 0.1.0 and `brotli` 8 (BSD-3-Clause AND
      MIT, the full compressor; sheaf's lock has only
      `brotli-decompressor`).
    - **`load_embedded` bundles** (`src/bcmap/embedded.rs`) 65 CMaps in
      one 250,740-byte brotli blob: every name in ISO 32000-1 Table 118
      (61, Identity-H/V and H/V included) plus `Adobe-CNS1-UCS2`,
      `Adobe-GB1-UCS2`, `Adobe-Japan1-UCS2` and `Adobe-Korea1-UCS2`.
      Anything else returns `None`. The first use decompresses the whole
      bundle into one static `Vec`; its size is unverified.
    - Gaps: its codespace ranges are `pub(crate)` and unused ("Acrobat
      seems to ignore it"), so `lookup_cid_code(code, byte_len)` leaves
      the code length to the caller, by shortest match. `usecmap`
      recursion is capped (`MAX_NESTING_DEPTH`), but range counts aren't,
      beyond the input's size.
  - **Adobe cmap-resources**
    ([GitHub](https://github.com/adobe-type-tools/cmap-resources)): BSD-3;
    pushed 2023-11-15. Raw PostScript CMaps for every collection
    (Japan1-7, GB1-6, CNS1-7, Korea1/KR-9…). Data, not a crate.
  - **pdf.js bcmaps**
    ([external/bcmaps](https://github.com/mozilla/pdf.js/tree/master/external/bcmaps)):
    168 `.bcmap` files, 1,165,667 bytes, with Adobe's LICENSE, loaded on
    demand by URL. More than Table 118 (the UTF32 and 2004 variants), in
    Mozilla's own format. The approach to copy if Table 118 is too small.
- **Fit:** hayro-cmap supplies the data and the code → CID → Unicode
  lookups sheaf lacks, in safe Rust, already compressed. sheaf's own
  parser keeps `ToUnicode` and its `Limits` (hayro-cmap takes none).
  Mixed-width CMaps (RKSJ: 1- and 2-byte codes) work by shortest match; a
  code valid in the codespace but unmapped may split differently from
  PDFium, so corpus-check it.
- **Recommendation:** **build on hayro-cmap:** its bundle and CID →
  Unicode, sheaf's ToUnicode parser kept, and an upstream PR exposing
  codespace ranges, then revisit using it for both. Medium. What would
  change it: CJK files in the binders naming CMaps outside Table 118
  (then Adobe's full set, pdf.js-style).

#### 27. The standard 14 fonts' metrics

- **Need:** widths, glyph boxes, ascender and descender, and built-in
  encodings for the 14 base fonts.
- **Where:** M10.5 (done); M11 slice 3; M10 slice 6.
- **Today:** built. `crates/sheaf-doc/src/content/font/afm.rs` parses
  Adobe's 14 AFMs (`assets/afm`, `assets/LICENSE-afm`, also in
  `packaging/licenses`); `std14.rs` has exact encoders; the interpreter
  branch adds boxes, `Ascender`/`Descender` and `FontBBox`.
  `m10.md:590-595` is stale ("the full AFM tables, and the other 12
  faces, are new"; `stamps.rs:207-254` is gone).
- **Candidates:** none worth taking; each has one maintainer and few
  downloads, so none beats sheaf's ~100 lines plus Adobe's files:
  [`afm`](https://crates.io/crates/afm) 0.1.2 (2018, 6.6k downloads),
  [`pdf-core-14-font-afms`](https://crates.io/crates/pdf-core-14-font-afms)
  0.1.1 (2024, 3.6k),
  [`pdf-base14-metrics`](https://crates.io/crates/pdf-base14-metrics) and
  [`adobe-font-metrics`](https://crates.io/crates/adobe-font-metrics)
  0.0.2 (2026-09, under 3k each).
- **Recommendation:** **build in-house (keep).** High. PDFium draws
  unembedded standard 14 fonts with its own Foxit substitutes, so its
  boxes can differ from AFM boxes; slice 3's tolerance absorbs that.

#### 28. Outlines of redacted glyphs (M11's leak; M14's re-subsetting)

- **Need:** after redaction, glyphs whose codes appear nowhere shouldn't
  keep their outlines in the program.
- **Where:** M11 slice 5; M14 optimize. D11.
- **Today's plan:** "`/ToUnicode` and `/Widths` entries for character
  codes no longer used anywhere in the document" are removed
  (`m11.md:174-178`). "**Re-subsetting fonts** … M14 optimize. Until then
  a font subset can show that a character occurred, though not where.
  Documented" (`:562-564`; docs at `:536-537`).
- **Candidates:** no crate re-subsets the bare CFF or Type 1 programs
  PDFs carry (entry 62).
- **Fit:** blanking in place is much smaller than subsetting:
  - TrueType: a zero-length `glyf` entry (rewrite `loca`);
  - CFF: the charstring becomes `endchar` (rebuild the CharStrings
    INDEX);
  - Type 1: the same, under eexec.

  It keeps every glyph ID, code, width and table. Content streams don't
  change, so edits stay byte-preserving; the result is a new font-file
  stream. read-fonts parses all three; the writers are sheaf's
  (write-fonts covers SFNT tables only, and nothing writes Type 1). It
  needs the "codes no longer used anywhere" set slice 5 already computes.
- **Recommendation:** **build in-house on read-fonts.** Medium. Consider
  moving it into M11 slice 5, which closes the documented leak for the
  redacted copy (Greg's call); M14's size-driven re-subsetting reuses it.

#### 29. The Standard security handler: read R2–R6, keep on save, write AES-256

- **Need:** decrypt and encrypt every Standard-handler revision (RC4
  40–128; AES-128, V4; AES-256, R5, Adobe's deprecated extension, and
  R6); authenticate user and owner passwords; read and keep `/P`; write
  new R6 encryption from Security….
- **Where:** M11 slices 1–2 and slice 1's leftovers; F13.
- **Today's plan:** lopdf's handler, with sheaf's serializer encrypting
  each object. "Every save keeps a file's encryption exactly"
  (`m11.md:211`); "Kept on every save since M10.5 … `serialize/crypt.rs`
  writes each object with the file's key" (`:216-218`); "New encryption
  is AES-256 (R6) only" (`:219`), "written with `/Length 256`"
  (`:221-225`, `:442`); "Work around lopdf #584 for reading" (`:424`).
  F13: "encryption, which sheaf would keep on the RustCrypto crates and
  not hand-roll" (`backlog.md:702-703`).
- **What sheaf and lopdf use today** (vendored lopdf at `39492cb`):
  - lopdf: aes 0.9.3, cbc 0.2.1, ecb 0.2.1 (R6's `/Perms`), md-5 0.11.0,
    sha2 0.11.0 (R6's Algorithm 2.B); its own RC4
    (`vendor/lopdf/src/encryption/rc4.rs`, 66 lines); stringprep 0.1.5
    (SASLprep for R5/R6 passwords); rand 0.10 and getrandom 0.4.
  - sheaf-doc: hmac 0.13 and sha2 0.11 for deterministic AES IVs
    (`serialize/crypt.rs:15-45`); zeroize 1.9.1 for passwords
    (`store.rs:42`).
  - The corpus covers every revision. `parsing/` alone has rc4-40 R2/R3,
    rc4-64/80/128, aes-128 R4, aes-256 R5 (36 files) and R6 (44).
- **Candidates:**

  | Crate | Latest | License | Maintenance | Notes |
  |---|---|---|---|---|
  | [aes](https://crates.io/crates/aes) | 0.9.3, 2026-08-28 | MIT OR Apache-2.0 | RustCrypto (newpavlov); 370M downloads | Pure Rust, no_std; AES-NI by runtime detection |
  | [cbc](https://crates.io/crates/cbc) / [ecb](https://crates.io/crates/ecb) | 0.2.1, 2026-05-20 / 0.2.1, 2026-08-21 | MIT OR Apache-2.0 | RustCrypto | Pure Rust |
  | [md-5](https://crates.io/crates/md-5) | 0.11.0, 2026-03-27 | MIT OR Apache-2.0 | RustCrypto; 417M downloads | Pure Rust |
  | [sha2](https://crates.io/crates/sha2) | 0.11.1, 2026-10-12 | MIT OR Apache-2.0 | RustCrypto (tarcieri, newpavlov); 1.1B downloads | Pure Rust plus intrinsics |
  | [rc4](https://crates.io/crates/rc4) | 0.2.0, 2026-03-30 | MIT OR Apache-2.0 | RustCrypto; 1.6M downloads | Pure Rust; would replace lopdf's own RC4 |
  | [stringprep](https://crates.io/crates/stringprep) | 0.1.5, 2024-05-25 | MIT/Apache-2.0 | sfackler alone; 245M downloads (postgres' tree) | Pure Rust |

- **Fit:** complete for the Standard handler; the gaps are in lopdf's
  logic, not the primitives.
  - **#584 (the owner password giving the wrong key) is fixed in the
    vendored code.** The fix is upstream `0e781ce` ("derive the file
    encryption key from the recovered user password", PR #573,
    2026-09-12), an ancestor of the vendored `39492cb`, and it ships in
    lopdf 0.46.0. `EncryptionState::decode` calls
    `resolve_password_for_key_derivation`
    (`vendor/lopdf/src/encryption.rs:623-624`, `algorithms.rs:985-1001`).
    [Issue #584](https://github.com/J-F-Liu/lopdf/issues/584) still shows
    open.
  - **#603** is fixed in 0.46.0 (`2f497f8`, 2026-10-20), though its
    [issue](https://github.com/J-F-Liu/lopdf/issues/603) also shows open.
  - Hygiene: lopdf's `EncryptionState` keeps the file key in a plain
    `Vec`, and sheaf's `Crypt` keeps an HMAC keyed with it; neither is
    zeroized (F19).
- **Recommendation:** **build on RustCrypto**, as today. High. When F13
  brings the handler in-house, write Algorithms 2, 2.A, 2.B, 6 and 7 on
  these crates, with `Zeroizing` keys. Swapping lopdf's RC4 for the rc4
  crate, or keeping its 66 lines, are both fine. AES-GCM (ISO/TS 32003)
  isn't planned; RustCrypto's aes-gcm would cover it.

#### 30. Redaction hit tests: glyphs, image areas, annotations

- **Need:** decide what a mark touches: which glyph boxes, shrunk 10%,
  overlap it; which image areas, paths and annotations are wholly or
  partly under it; for verification, whether any of PDFium's character
  boxes in the output overlaps one. Marks are rectangles or `QuadPoints`
  quads; through a CTM they stay convex quadrilaterals. "Content outside
  the crop box" is a page minus a rectangle: at most four rectangles.
- **Where:** M11 slices 5–7 and 9.
- **Today's plan:** build. "every glyph whose box, shrunk by 10% on each
  side, overlaps the mark" (`m11.md:137`); "sheaf reopens the copy it
  wrote with PDFium and checks that no character box intersects a mark"
  (`:202-203`); "content outside the crop box (redacted as the area
  outside it)" (`:249`).
- **Candidates:** [geo](https://crates.io/crates/geo) 0.33.1 (2026-04-20;
  MIT OR Apache-2.0; georust; 24.7M downloads) has `Intersects` and
  `Contains`; [i_overlay](https://crates.io/crates/i_overlay) 9.1.0 has
  spatial predicates (entry 31); [rstar](https://crates.io/crates/rstar)
  0.13.0 (2026-05-24; MIT OR Apache-2.0; georust; 51M downloads) is an
  R*-tree.
- **Fit:** every test is convex against convex: a separating-axis test,
  about 30 lines. The work is glyphs times marks, on marked pages only,
  so a 7,000-page file costs nothing on pages without marks. A
  search-and-redact run might put 50 marks on a dense page; brute force
  handles that. An R-tree pays off only with hundreds of marks a page,
  which isn't measured.
- **Recommendation:** **build in-house** in sheaf-doc's `geom`. High. Add
  rstar only if a measurement shows the need.

#### 31. Clipping paths partly under a mark (polygon booleans)

The security section's detail; core's note that both crates are MIT OR
Apache-2.0 settles the plan's license check.
- **Need:** keep what's outside the marks of a painted path partly under
  them: a fill under its rule (nonzero or even-odd), a stroke whose ink
  must not survive under the mark, or both. The result is written back as
  path operators; a clip path can't be used instead. Only paths that
  intersect a mark are rewritten, and every other byte is kept. Input is
  untrusted: coordinates can be NaN, ±1e38, or millions of segments.
- **Where:** M11 slice 6; slice 9's "content outside the crop box". D6.
- **Today's plan:** "Partly covered paths are clipped geometrically,
  keeping what's outside the mark. Curves are flattened only in the
  paths that get clipped, with a polygon-boolean crate such as `geo` or
  `i_overlay` (licenses to check)" (`m11.md:160-163`); "Path clipping and
  the crate for it" (`:500`).
- **Candidates:**

  | Crate | Latest | License | Maintenance | Build | Fit notes |
  |---|---|---|---|---|---|
  | [i_overlay](https://crates.io/crates/i_overlay) ([GitHub](https://github.com/iShape-Rust/iOverlay), 215★) | 9.1.0, 2026-10-14 | MIT OR Apache-2.0; its i_float and i_shape are MIT | Nail Sharipov alone; 132 versions since 2023-08; majors 8→9 in 2026-09; 10.4M downloads; powers geo's booleans | Pure Rust, `no_std`; `unsafe` only for `get_unchecked` (77 sites); rayon optional | Union, intersection, difference, xor; fill rules EvenOdd, NonZero, Positive, Negative; f32/f64 APIs over an integer grid; polyline clip with `ClipRule { invert, boundary_included }`; stroke and outline offsetting. Its README: "Infallible APIs panic on invalid bounds" (non-finite values, or past 2^500 for f64); the fixed-scale APIs return errors |
  | [geo](https://crates.io/crates/geo) | 0.33.1, 2026-04-20 | MIT OR Apache-2.0 | georust; active | Pure Rust | `BooleanOps` **pins i_overlay `>=4.5.1, <4.6.0`**, five majors behind; brings geo-types, rstar, robust, spade, earcut and more. A GIS API for no gain |
  | [clipper2-rust](https://crates.io/crates/clipper2-rust) ([GitHub](https://github.com/larsbrubaker/clipper2-rust), 28★) | 1.2.0, 2026-09-18 | **BSL-1.0** (Boost: permissive) | Lars Brubaker; created 2026-02; 155k downloads | Pure Rust port of Angus Johnson's Clipper2; 1 `unsafe`; only num-traits; a WebAssembly demo | `difference_d` with fill rules, open (stroke centerline) subjects, `rect_clip`, offsetting. Claims "exact behavioral match with C++" over 457 tests |
  | [clipper2](https://crates.io/crates/clipper2) | 0.6.0, 2026-05-06 | MIT OR Apache-2.0 | tirithen | FFI to C++ Clipper2 | A C++ build; the pure port is better |
  | [geo-clipper](https://crates.io/crates/geo-clipper) | 0.9.0, 2025-02-08 | ISC | lelongg | FFI to Clipper 1 | Old engine; no |
  | [linesweeper](https://crates.io/crates/linesweeper) | 0.5.0, 2026-10-05 | MIT OR Apache-2.0 | Joe Neeman; 99k downloads; its [GitHub repo](https://github.com/jneem/linesweeper) is **archived** (last push 2026-01-25), yet releases continue (where it's developed is unverified) | Pure Rust, no `unsafe`, on kurbo | Booleans on Bézier paths **without flattening**, EvenOdd and NonZero; "Currently in an early beta state" |
  | [flo_curves](https://crates.io/crates/flo_curves) | 0.8.1, 2026-08-25 | Apache-2.0 | Logicalshift; 376k downloads | Pure Rust | Bézier booleans; not evaluated further |
  | lyon | 1.0.19 | MIT OR Apache-2.0 | nical | Pure Rust | Tessellation; no booleans |

- **Fit:**
  - **Fills:** a difference under the original fill rule handles
    self-intersecting, multi-subpath PDF fills. The output is simple
    contours with holes, so it can be written back with nonzero `f`
    whatever the original rule; confirm in the spike.
  - **Strokes:** no crate clips "a stroke's ink" directly. (a) Expand
    the stroke to its outline with kurbo's `stroke` (joins, caps, miter
    limit, dashes; `kurbo-0.13.1/src/stroke.rs:260`), take the
    difference and fill it: exact, but the stroke becomes a fill, which
    M13's editing would see. (b) Clip the centerline with i_overlay's
    inverted string clip, or clipper2's open subjects, against marks
    grown by half the line width under the CTM and the miter limit: the
    stroke stays a stroke, with new butt ends at the cuts. A choice for
    kickoff (D6).
  - **Hostile input:** validate coordinates (finite, bounded) and cap the
    segment count before the float API, or use the fixed-scale APIs,
    because the infallible ones panic.
  - **Churn:** four i_overlay majors this year, so pin it exactly.
- **Recommendation:** **adopt i_overlay directly, behind a one-function
  sheaf-doc module** (path minus marks), with kurbo for flattening and
  stroke expansion. Medium. Run clipper2-rust beside it in the corpus
  tier: the two kept areas must agree within tolerance. Watch
  linesweeper (curves kept as curves, but beta and archived). Not geo.

#### 32. Curve flattening and stroke expansion

- **Need:** flatten Béziers to a tolerance in device terms (user-space
  tolerance over the CTM's scale); expand strokes with dashes and joins
  (entry 31's option a); affine transforms.
- **Where:** M11 slice 6.
- **Today's plan:** "Curves are flattened only in the paths that get
  clipped" (`m11.md:161-162`), no crate named.
- **Candidates:** [kurbo](https://crates.io/crates/kurbo) 0.13.1
  (2026-05-13; Apache-2.0 OR MIT; Linebender, 9 owners including Raph
  Levien; 57M downloads; 1,022★; pure Rust, `no_std` with libm), with
  `flatten` (`bezpath.rs:622`), `stroke`, `dash` and `Affine`, **already
  in the lockfile** through usvg 0.46;
  [lyon_geom](https://crates.io/crates/lyon_geom) 1.0.20 (2026-10-20; MIT
  OR Apache-2.0; nical; 13M downloads), with
  `CubicBezierSegment::flattened`, in the lockfile through gpui-pre's
  lyon.
- **Fit:** both flatten; only kurbo expands strokes, and linesweeper
  builds on kurbo.
- **Recommendation:** **adopt kurbo.** High.

#### 33. PDF image samples, in the image's own color space

The images section's detail; the security section deferred to it and
added two notes, kept below.
- **Need:** an image XObject's or inline image's samples in its own color
  space, bit depth and `/Decode`, so filling a region and re-encoding
  changes nothing else.
- **Where:** M11 slice 6; the same layer serves M13 (image edit, replace,
  crop), M14 (downsampling, recompression, PDF/A re-encoding), slice 6's
  check, and M5's separator fallback, "decoding a scanned page's own
  image instead of drawing it" (`m5-intake.md:934`). D5.
- **Today's plan:** "A partly covered image is decoded, its pixels under
  the mark are replaced with the fill color, and it's encoded again.
  PDFium decodes, since it handles every filter. Bilevel images become
  CCITT G4 (`sheaf-image`), JPEGs become JPEG again, and the rest Flate"
  (`m11.md:150-157`). Slice 6's check: "pixels under marks are the fill
  color in the image data itself" (`:503-505`). Unbuilt.
- **Why PDFium doesn't fit** (its source):
  - [`FPDFImageObj_GetBitmap`](https://pdfium.googlesource.com/pdfium/+/HEAD/fpdfsdk/fpdf_editimg.cpp)
    `Realize`s or `ConvertTo`s the image into 8bpp gray or BGR/BGRx/BGRA.
    1-bit images come back 8-bit, paletted ones BGR, masks are ignored,
    and CMYK, ICCBased, Separation, DeviceN, Indexed and 16-bit images
    come back converted. Re-encoding those pixels turns every redacted
    image into DeviceRGB or DeviceGray: print colors shift, spot colors
    are lost, and a PDF/A file can stop conforming.
  - `FPDFImageObj_GetImageDataDecoded` goes through `PDF_DataDecode`,
    which stops at the last filter it takes for an image decoder ("assume
    it's an image decoder",
    [`fpdf_parser_decode.cpp`](https://pdfium.googlesource.com/pdfium/+/HEAD/core/fpdfapi/parser/fpdf_parser_decode.cpp)),
    so for DCT, JPX, JBIG2 and CCITT it returns still-encoded bytes.
  - Every call also goes through sheaf's global PDFium lock.
- **Candidates:** one per filter (entries 34–38). lopdf covers the stream
  filters with caps (`vendor/lopdf/src/object.rs:893-898`; predictors at
  `:1119-1150`).
  - `hayro-syntax` 0.8.0 wires exactly this set behind its `images`
    feature (zune-jpeg, hayro-ccitt, hayro-jbig2, hayro-jpeg2000), but
    its `filter::Filter::apply` is crate-private, so sheaf can't call it
    on lopdf objects. Its `filter/*.rs` (2,132 lines, MIT OR Apache-2.0)
    is the reference for the PDF quirks: DCT `/ColorTransform` and Adobe
    APP14; JPEGs whose headers give wrong dimensions; CCITT
    `DecodeParms`; JBIG2 globals; JPX `SMaskInData`.
- **Fit:**
  - Every crate is pure Rust, safe at the crate level, permissive, and
    lets sheaf read the dimensions before decoding, to cap the work.
    Each builds for wasm: hayro's demo viewer runs in the browser; the
    others are `no_std` or pure Rust.
  - pdf_oxide (MIT OR Apache-2.0, 1.35M downloads) uses nearly the same
    stack: `fax` ^0.3, `jpeg-decoder`, `hayro-jbig2`, `hayro-jpeg2000`,
    optional `lcms2` and `fast_image_resize`.
  - **How often each filter occurs** (a byte search for each name over
    `/home/greg/repos/pdf-corpus`, `pool/` excluded):

    | Category | DCT | CCITT | JBIG2 | JPX | LZW |
    |---|---|---|---|---|---|
    | `scans/` (709 files) | 464 | 150 | 127 | 52 | 2 |
    | `forms/` (4,392) | 1,521 | 189 | 17 | 89 | 37 |
    | `parsing/` (5,253) | 1,060 | 286 | 93 | 95 | 93 |

  - **Security's notes:** replace pixels in raw samples in the image's
    own color space and depth (CMYK, Indexed, 16-bit, ICC), never in a
    rendered BGRA bitmap. Re-encoding a JPEG loses quality over the whole
    image, not only under the mark, so the report says which images were
    re-encoded.
- **Recommendation:** **build on lopdf, zune-jpeg, hayro-ccitt,
  hayro-jbig2 and hayro-jpeg2000,** in one `sheaf-image` module (pixels
  in, pixels out) with a small decoder trait, so PDFium's bitmap can be
  a cross-check in tests. PDFium stays the renderer that checks the
  output. High on the shape; medium on hayro-jbig2 ("under
  development").

#### 34. DCT (JPEG) decode

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

#### 35. DCT (JPEG) encode

- **Need:** re-encode a redacted or downsampled JPEG in gray, RGB or CMYK
  (Adobe-inverted CMYK included), ideally with the original's
  quantization tables and subsampling, to limit generation loss.
- **Candidates:**
  - image's `JpegEncoder` (in use): **only `L8` and `Rgb8`**; every other
    type is `Unsupported` (`image-0.25.10/src/codecs/jpeg/encoder.rs:494-510`).
  - [`jpeg-encoder`](https://crates.io/crates/jpeg-encoder) 0.7.1
    (2026-07-27; vstroebel alone; 9M downloads). License `(MIT OR
    Apache-2.0) AND IJG`: the IJG terms ask the docs to say the software
    is "based in part on the work of the Independent JPEG Group" (F18).
    `no_std`, no `unsafe` unless `simd`. 1-, 3- and 4-component color
    (CMYK, YCCK), custom quantization tables, chroma subsampling,
    progressive, optimized Huffman tables; no dependencies.
  - [`mozjpeg`](https://crates.io/crates/mozjpeg) 0.10.13 (2025-02-18;
    IJG) on `mozjpeg-sys` 2.2.3 (IJG AND Zlib AND BSD-3-Clause): C, needs
    nasm for SIMD; compresses better.
  - [`turbojpeg`](https://crates.io/crates/turbojpeg) 1.5.1 (2026-07-25;
    Unlicense OR MIT): C (libjpeg-turbo), with lossless transforms.
  - No crate does a DCT-domain wipe of a region (jpegtran-style), which
    would leave the rest untouched; `dct-io` 0.1.1 only reads and writes
    coefficients.
- **Fit:** jpeg-encoder covers CMYK and can reuse the source's tables, but
  it's one person's crate. Re-encoding must not copy APPn segments: an
  EXIF thumbnail, XMP or a Photoshop resource can hold a preview of the
  unredacted image.
- **Recommendation:** **adopt jpeg-encoder** for M11, M13 and M14
  re-encoding. Keep image's encoder where it's used today (the
  byte-identical rule). mozjpeg only if M14 measures a large saving.
  Medium-high.

#### 36. JPX (JPEG 2000) decode

- **Candidates:**
  - [`hayro-jpeg2000`](https://crates.io/crates/hayro-jpeg2000) 0.4.1
    (2026-10-04; 11 releases since 2025-12; MIT OR Apache-2.0; LaurenzV;
    4.7M downloads). `#![forbid(unsafe_code)]`, but its default `simd`
    feature uses `fearless_simd`, which has unsafe; `no_std`. "tested
    against 20.000+ images scraped from random PDFs" and "a large part of
    the OpenJPEG test suite"; "expect … worse performance than OpenJPEG,
    but the difference gap should not be too large"
    ([README](https://github.com/LaurenzV/hayro/tree/main/hayro-jpeg2000)).
    **It yields 8-bit only**, so 16-bit JPX drops to 8-bit.
    `DecodeSettings::target_resolution` decodes at reduced size, useful
    for M14.
  - [`openjpeg-sys`](https://crates.io/crates/openjpeg-sys) 1.0.12
    (2025-04-16; BSD-2-Clause): C, built from source.
    [`jpeg2k`](https://crates.io/crates/jpeg2k) 0.10.1 (2025-07-29; MIT
    or Apache-2.0) wraps openjpeg-sys or `openjp2`.
    [`openjp2`](https://crates.io/crates/openjp2) 0.6.1 (2025-07-17;
    BSD-2-Clause): a c2rust port with heavy `unsafe` (count unverified).
  - [`jpeg2000`](https://crates.io/crates/jpeg2000) 0.3.0: **GPL-3.0,
    out**.
  - Encoders: only OpenJPEG through the C crates or openjp2; nothing in
    safe Rust.
- **Recommendation:** **adopt hayro-jpeg2000 for decoding.** Re-encode a
  redacted JPX as Flate (lossless; PDF/A-1 doesn't allow JPX anyway); for
  M14's lossy recompression, JPEG. No JPX encoder. High (decode); medium
  (no encoder).

#### 37. JBIG2 decode

- **Candidates:**
  - [`hayro-jbig2`](https://crates.io/crates/hayro-jbig2) 0.3.1
    (2026-10-04; four releases since 2026-01; MIT OR Apache-2.0;
    LaurenzV). `forbid(unsafe_code)`; `simd` through `fearless_simd`
    only for halftones; `no_std`. `Image::new_embedded(data, globals)`
    takes PDF's `/JBIG2Globals`. Its README: "This crate is currently
    under development."
  - [`jbig2dec`](https://crates.io/crates/jbig2dec) 0.3.1 (2020): the
    crate is GPL-3.0 and jbig2dec itself AGPL. **Out.**
  - Encoders: [`jbig2enc`](https://crates.io/crates/jbig2enc) 0.1.2
    (Apache-2.0, 301 downloads) and
    [`jbig2enc-rust`](https://crates.io/crates/jbig2enc-rust) 0.5.4 (MIT
    OR Apache-2.0, 530 downloads), both first released in 2026: too
    young. Lossy symbol matching is also what swapped digits in Xerox
    scanners' PDFs (2013), unacceptable on tax documents.
- **Fit, and a redaction detail:** `/JBIG2Globals` is a symbol dictionary
  several images can share; 15 of the corpus's 127 JBIG2 scans have one.
  Re-encoding the redacted image as G4 leaves the globals in the file
  whenever another image still points at them, and their symbols can
  include glyphs only the redacted area used.
- **Recommendation:** **adopt hayro-jbig2 for decoding; re-encode as G4;
  never encode JBIG2.** Drop a globals stream nothing else refers to;
  otherwise decide whether keeping it leaks, or re-encode every image
  that shares it (D5). Medium (the crate's maturity).

#### 38. CCITT G3 and G4

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

#### 39. PNG files, and Flate image streams

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

#### 40. Color: ICC profiles and output intents

- **Need:** convert a redaction fill or stamp color into an image's
  ICCBased or CMYK space (M11, M13); write a PDF/A output intent profile
  (M14, `ROADMAP.md:455`); read profile headers for preflight-lite's
  color checks (`:456`).
- **Today's plan:** none written.
- **Candidates:**
  - [`moxcms`](https://crates.io/crates/moxcms) 0.9.1 (2026-09-15; 36
    releases since 2025-02; BSD-3-Clause OR Apache-2.0; awxkee alone;
    107M downloads; used by image 0.25.10, hayro-interpret,
    typst-library and jxl-oxide). `forbid(unsafe_code)` unless its SIMD
    features are on. Reads and writes profiles (`writer.rs`'s `encode`);
    builds sRGB, Display P3, BT.2020 and gray profiles (`defaults.rs`).
    **Already in the lockfile** (0.8.1, through image).
  - [`lcms2`](https://crates.io/crates/lcms2) 6.2.0 (2026-08-26; MIT) on
    `lcms2-sys` 4.0.7 (MIT): C, Little CMS, the reference; PDFium has its
    own copy for rendering.
  - [`qcms`](https://crates.io/crates/qcms) 0.3.0 (2024-01-09; MIT;
    Firefox): narrower.
- **Fit:** re-encoding in the image's own space needs no color
  management; only the fill color does. moxcms can generate an sRGB
  output-intent profile itself. Whether it does RGB-to-CMYK through a
  profile's B2A tables is unverified.
- **Recommendation:** **adopt moxcms** (already there); lcms2 only if
  preflight needs exact Little CMS behavior. Medium. The output intent's
  profile is D14: the core section would ship CC0 profiles instead
  (entry 68).

#### 41. XMP: sanitize (M11), PDF/A and PDF/UA (M14, M17)

Two sections, two needs, and a split recommendation (D12).
- **Needs:**
  - **M11 sanitize** (security): remove a file's XMP (the catalog's
    `/Metadata`, and page, image and Form XObject `/Metadata`), or write a
    minimal fresh packet that keeps PDF/A and PDF/UA claims
    (`pdfaid:part`/`conformance`, `pdfuaid:part`) and sets Producer and
    ModifyDate. Read the claims first.
  - **M14 and M17** (core): create or update the catalog's `/Metadata`:
    `pdfaid:part` and `conformance`, `pdfuaid:part` (and `rev` for
    UA-2), `dc:title` agreeing with Info, dates and Producer, keeping the
    other schemas a file has.
- **Where:** M11 slice 9 (proposal 12, bug 9); M14 PDF/A; M17 PDF/UA;
  Pending "Document properties and the initial view"
  (`ROADMAP.md:891-897`); B8 (closed by #35: the comment summary now
  writes its `/Info` title, Producer and CreationDate, with no op for
  document info yet).
- **Today's plan:** "metadata (Info, XMP, `/PieceInfo`)" (`m11.md:242`);
  bug 9: "`/Info`, XMP and `/PieceInfo` kept as they were, with `ModDate`
  and `Producer` never updated" (`:374-375`). The plan doesn't say what
  happens to a PDF/A file's claim. Nothing beyond mentions for M14.
- **Candidates:**
  - [quick-xml](https://crates.io/crates/quick-xml) 0.42.0 (2026-08-22;
    MIT; tafia, Mingun, dralley; 492M downloads; pure Rust). **sheaf-doc
    uses 0.41** for XFDF, XFA and the XMP conformance claims.
  - [xmp-writer](https://crates.io/crates/xmp-writer) 0.3.3 (2026-04-21;
    MIT OR Apache-2.0; typst's laurmaedje and reknih; 4.6M downloads; 6
    releases; no dependencies, no `unsafe`). Write-only, "a simple API to
    write XMP metadata": `pdfa_part`, `pdfa_conformance`, `pdfua_part`,
    `producer`, `modify_date` (`src/lib.rs:664-751`).
  - [xmp_toolkit](https://crates.io/crates/xmp_toolkit) 1.12.1
    (2026-03-19; MIT OR Apache-2.0 bindings; Adobe; 233k downloads):
    Adobe's **C++ XMP Toolkit SDK (BSD-3-Clause), with expat (MIT) and
    zlib bundled**, built with `cc`. 2.1 MB; a Windows CRT feature; no
    wasm.
- **Fit:**
  - Sanitizing deletes, or writes a fresh minimal packet; quick-xml plus
    xmp-writer cover both (security).
  - Conversion must update existing packets (Acrobat and Word put their
    own namespaces in them, which a conversion shouldn't drop), and the
    PDF/A and PDF/UA properties are a few dozen lines of XML. Adopting
    xmp-writer only for new packets means two code paths (core).
  - xmp_toolkit is C++ for a round-trip editor sheaf doesn't need.
- **Recommendation:** security: **build on quick-xml to read and
  xmp-writer to write** (high). Core: **build in-house on quick-xml**
  (read, patch, write), with xmp-writer's schema coverage as a checklist
  (medium). Both: not xmp_toolkit. This memo recommends core's single
  path (D12). Either way the plan needs a line on PDF/A claims.

#### 42. JavaScript, actions and scripts: stripping

- **Need:** remove `/OpenAction`; `/AA` on the catalog, pages,
  annotations and fields; `/Names /JavaScript`; JavaScript, Launch, URI,
  SubmitForm, ImportData, GoToR, GoToE and RichMedia actions, following
  their `/Next` chains; field scripts; XFA `<script>` elements in the
  template packet. No dangling references.
- **Where:** M11 slice 9; M10's handoff.
- **Today's plan:** build. "links, actions and JavaScript (`/OpenAction`,
  `/AA`, `/Names /JavaScript`, Launch, URIs)" (`m11.md:250-251`);
  "Sanitize removes field and document scripts and offers to flatten"
  (`m10.md:1141-1142`).
- **Candidates:** none over lopdf objects. pdf_oxide's
  `redaction/sanitize.rs` works in its own object model: prior art only.
- **Recommendation:** **build in-house** over the M10.5 emission walk,
  with quick-xml for XFA scripts. High.

#### 43. Passwords in memory, and remembering them

- **Need:** passwords, and later keys and PINs, wiped when dropped and
  kept out of logs and `Debug`. Remembering a document password in the
  OS keychain is something **no plan asks for** (ROADMAP.md, m10.md,
  m11.md, agent.md, the backlog and the prior-art notes checked).
- **Where:** M11 slices 1–2; M12 (PKCS#12 passwords, token PINs).
- **Today:** `Zeroizing<String>` for a file's password
  (`crates/sheaf-doc/src/store.rs:42`). Copies that can't be wiped:
  GPUI's text input, PDFium's C string, lopdf's file key (F19). GPUI
  0.3.9 already has `write_credentials`, `read_credentials` and
  `delete_credentials` (`gpui-pre-0.3.9/src/app.rs:1789-1804`): oo7's
  Secret Service on Linux, the Keychain on macOS, Credential Manager on
  Windows.
- **Candidates:** [zeroize](https://crates.io/crates/zeroize) 1.9.1
  (2026-10-06; Apache-2.0 OR MIT; RustCrypto; 801M downloads), in use;
  [secrecy](https://crates.io/crates/secrecy) 0.10.3 (2024-10-09;
  Apache-2.0 OR MIT; iqlusion; 186M downloads), which **cryptoki already
  depends on** for PINs (`AuthPin`);
  [keyring](https://crates.io/crates/keyring) 4.2.0 (2026-08-29; MIT OR
  Apache-2.0; 34M downloads), now keyring-core plus per-platform stores.
- **Recommendation:** **keep zeroize; adopt secrecy in M12** for PINs and
  PKCS#12 passwords (it comes with cryptoki). High. Document that wiping
  is best-effort. If remembering ever comes up: GPUI's credentials API
  in the app, keyring only for the CLI. High.

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

#### 44. CMS SignedData: parse, build, verify

- **Need:** parse `/Contents` in BER or DER, ignoring the zero padding.
  Build detached SignedData for adbe.pkcs7.detached and
  ETSI.CAdES.detached from an external digest (signed attributes:
  contentType, messageDigest, signingCertificateV2, optionally Adobe's
  revocation archive; unsigned: the signature timestamp), with keys sheaf
  can't see (tokens, CNG, the Keychain). Verify the digest against
  messageDigest, the signature over the DER signed attributes, and the
  ESS signing-certificate binding.
- **Where:** M12 slices 2, 3 and 5; B5. D2.
- **Today's plan:** no crate named. "Digital signatures: sign with
  PKCS#12 files, the Windows certificate store, and PKCS#11 tokens"
  (`ROADMAP.md:397-398`); "Signature panel: validation status,
  certificate details, changes since signing" (`:399-400`).
- **Candidates:**

  | Crate | Latest | License | Maintenance | Build | Notes |
  |---|---|---|---|---|---|
  | [cms](https://crates.io/crates/cms) ([RustCrypto formats](https://github.com/RustCrypto/formats), 343★) | stable **0.2.3, 2024-01-08** (der 0.7); **0.3.0-rc.0, 2026-10-13** (der 0.8) | Apache-2.0 OR MIT | tarcieri plus the org; 15M downloads | Pure Rust, `no_std`, `forbid(unsafe_code)` | Types for SignedData, EnvelopedData (KeyTrans, KeyAgree, KEM), TimeStampedData; builders for SignedData (`SignerInfoBuilder` with an external digest, taking `signature::Signer` keys) and EnvelopedData. **No verification API.** 0.3 decodes BER (`der` feature `ber`) |
  | [openssl](https://crates.io/crates/openssl) | 0.10.81, 2026-06-12 | Apache-2.0 (OpenSSL 3 is Apache-2.0) | sfackler, alex; 435M downloads | **FFI to OpenSSL**; a vendored C build on Windows and macOS | CMS sign and verify, PKCS#12, OCSP, an X509 store. A C parser in the path of untrusted input, and a heavy cross-platform build |
  | [rasn-cms](https://crates.io/crates/rasn-cms) | 0.29.0, 2026-10-05 | MIT OR Apache-2.0 | XAMPPRocky alone; 4.5M downloads | Pure Rust; rasn codecs include BER | Types only, in its own ecosystem, apart from x509-cert |
  | [cryptographic-message-syntax](https://crates.io/crates/cryptographic-message-syntax) | 0.28.0, 2025-08-17 | **MPL-2.0** | indygreg; 6M downloads; last release 14 months ago | Pure Rust (bcder, ring) | Sign, verify and a TSP client, built for Apple code signing. MPL-2.0 is a cost |

- **The B5 spike over the corpus:** cms 0.2.3 failed "indefinite length
  disallowed" on **138 of 593**, and one more on a duplicate in a SET OF
  (454 parsed). cms 0.3.0-rc.0 decoded the whole SignedData, with every
  certificate through x509-cert 0.3, for all but one blob, which uses a
  `CertificateChoices` `[1]` alternative (an old attribute certificate)
  cms 0.3 doesn't define (`cms-0.3.0-rc.0/src/cert.rs:30-35`; F20).
- **Fit:**
  - cms 0.3 is the only pure-Rust option that reads real-world PDF CMS
    and builds SignedData with keys held elsewhere.
  - Verification is sheaf's own: compare digests, check the signature
    over the signed attributes (entry 45's primitives), check the ESS
    certificate hash.
  - **Timing:** the 0.3 line isn't finished. x509-ocsp 0.3.0-pre and
    x509-tsp 0.2.0-pre are on master, unpublished
    ([x509-ocsp](https://github.com/RustCrypto/formats/blob/master/x509-ocsp/Cargo.toml),
    [x509-tsp](https://github.com/RustCrypto/formats/blob/master/x509-tsp/Cargo.toml)).
    pkcs12 0.2.0-pre.0 pins `cms = "=0.3.0-pre.1"`, so **p12-keystore
    0.4.1 and cms 0.3.0-rc.0 don't resolve together** (`cargo
    generate-lockfile` fails).
- **Recommendation:** **build on cms 0.3, der 0.8 and x509-cert 0.3.**
  Medium-high: the design is clear, but the crates are release
  candidates. Pin exactly until the round is published; send upstream the
  missing `CertificateChoices` alternatives; re-check the line at
  kickoff.

#### 45. Signature primitives (verify; sign with in-memory keys)

- **Need:** verify RSA PKCS#1 v1.5 (with SHA-1 and 1024-bit keys, for old
  files), RSA-PSS, ECDSA P-256, P-384 and P-521, and Ed25519; sign with
  PKCS#12 keys, RSA and ECDSA. Platform-held keys are entries 49–51.
- **Where:** M12 slices 2–3; B5 (only for signatures with no signed
  attributes). D13.
- **Today's plan:** none.
- **Candidates:**
  - **RustCrypto** (pure Rust, `no_std`):
    [rsa](https://crates.io/crates/rsa) stable 0.9.10, 0.10.0-rc.19
    (2026-10-06), MIT OR Apache-2.0.
    **[RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html)
    (Marvin, timing) is unpatched in both** ("Still affected as of
    2026-09-12 … Local use on a non-compromised computer is fine");
    verifying isn't affected, since it uses no private key.
    [p256](https://crates.io/crates/p256) and
    [p384](https://crates.io/crates/p384) 0.14.0 (2026-07);
    [ecdsa](https://crates.io/crates/ecdsa) 0.17.0 (2026-07-02);
    [ed25519-dalek](https://crates.io/crates/ed25519-dalek) 3.0.0
    (2026-07-06; BSD-3-Clause); [sha1](https://crates.io/crates/sha1)
    0.11.0 and [sha1-checked](https://crates.io/crates/sha1-checked)
    (collision detection; 0.10.0, 0.11.0-rc.0).
  - **[ring](https://crates.io/crates/ring)** 0.17.14 (2025-03-11;
    Apache-2.0 AND ISC; Brian Smith; C and assembly). **Already in
    sheaf-app's normal build** through wasmtime-wasi-http's rustls. RSA
    PKCS#1 and PSS signing from PKCS#8 or DER keys; verifies with
    `RSA_PKCS1_1024_8192_SHA1_FOR_LEGACY_USE_ONLY`, P-256, P-384 (no
    P-521) and Ed25519. No RSA decryption.
  - **[aws-lc-rs](https://crates.io/crates/aws-lc-rs)** 1.18.2
    (2026-10-12; ISC AND (Apache-2.0 OR ISC); AWS; a C build through
    aws-lc-sys): adds PKCS#1 v1.5 RSA decryption
    (`Pkcs1PrivateDecryptingKey`) and legacy SHA-1 RSA. In the lockfile
    only, not in a normal build.
- **Fit:** RustCrypto verifies every algorithm in the corpus but GOST;
  use sha1-checked for SHA-1 digests, given the SHAttered PDFs. For
  PKCS#12 signing, the rsa crate is timing-unsafe in theory: acceptable
  on a desktop with no remote oracle, but an audit tool would flag it.
  ring is constant-time and already built.
- **Recommendation:** **adopt RustCrypto for verification** behind a
  verify function sheaf owns. Medium. For PKCS#12 signing, prefer
  **ring** in a `signature::Signer` adapter, so cms's builder takes it,
  over the rsa crate while Marvin is open. Decide at kickoff.

#### 46. X.509 parsing

- **Need:** parse signer, CA, TSA and OCSP-responder certificates, and
  CRLs, from CMS, `/DSS`, PKCS#12 files and the OS stores; read names,
  extensions, KeyUsage, EKU, AIA and CRL distribution points; tolerant
  enough for real files, and bounded.
- **Where:** M12 slices 2–5.
- **Candidates:** [x509-cert](https://crates.io/crates/x509-cert) 0.3.0
  (2026-07-09; Apache-2.0 OR MIT; RustCrypto; 49M downloads; `no_std`,
  `forbid(unsafe_code)`; a `crl` module and a builder);
  [x509-parser](https://crates.io/crates/x509-parser) 0.18.1 (2026-02-05;
  MIT OR Apache-2.0; rusticata's chifflier alone; 193M downloads;
  nom-based, a `verify` feature on ring; p12-keystore depends on it);
  [rasn-pkix](https://crates.io/crates/rasn-pkix) 0.29.0.
- **Fit:** x509-cert is what cms decodes into; the spike decoded every
  certificate in 592 SignedData. p12-keystore puts x509-parser in the
  tree as a second parser.
- **Recommendation:** **adopt x509-cert.** High.

#### 47. Chain validation for document signatures

- **Need:** build a path from the signer's certificate (the CMS's
  certificates, the DSS, optionally an AIA fetch) to a trust anchor;
  validate it at the signing time (the timestamp's, or the claimed time,
  flagged as such); check KeyUsage (digitalSignature or nonRepudiation);
  accept a document-signing EKU or none (anyExtendedKeyUsage,
  emailProtection, Adobe's Authentic Documents 1.2.840.113583.1.1.5,
  Microsoft's Document Signing 1.3.6.1.4.1.311.10.3.12, RFC 9336's
  id-kp-documentSigning 1.3.6.1.5.5.7.3.36); check revocation from the
  DSS, CMS archives or online; bound the work on hostile certificate
  bags.
- **Where:** M12 slice 3 (the verify panel) and slice 5 (LTV).
- **Candidates:**
  - **[rustls-webpki](https://crates.io/crates/rustls-webpki)**
    0.103.16 (2026-10-20; ISC; rustls' ctz and djc; 895M downloads;
    `no_std`, no `unsafe`; needs ring or aws-lc-rs for algorithms).
    `verify_for_usage(algs, anchors, intermediates, time, usage,
    revocation, …)` (`src/end_entity.rs:103-122`) takes any `UnixTime`.
    Its EKU check is a trait (`ExtendedKeyUsageValidator`) with
    `KeyUsage::required` and `required_if_present` for any OID. It checks
    CRLs (`RevocationOptions`) and budgets path building. Gaps: no OCSP;
    no certificate-policy processing; no end-entity KeyUsage-bit check
    found in its API (unverified); built-in algorithms need RSA ≥ 2048
    and have no SHA-1, though callers can pass their own
    `SignatureVerificationAlgorithm`; its hard-coded `DISTRUSTED_SPKIS`
    are TLS CAs (`verify_cert.rs:917`).
  - [synta-x509-verification](https://crates.io/crates/synta-x509-verification)
    0.5.1 (2026-10-17; MIT OR Apache-2.0; FreeIPA's abbra; 2k downloads):
    RFC 5280 §6 with pluggable crypto. Young.
  - [pkix-path](https://crates.io/crates/pkix-path) 0.3.2 (2026-06-15;
    Apache-2.0 OR MIT; Mark Atwood; 14k downloads). Young; provenance not
    checked.
  - tsp-ltv 0.6.0 (entry 52): chain building, trust stores, KeyUsage/EKU
    role checks. openssl's `X509_verify_cert` (entry 44's costs).
  - Prior art: [pyhanko-certvalidator](https://pypi.org/project/pyhanko-certvalidator/)
    0.33.0 (2026-10-17; MIT).
- **Fit:** none validates document-signing chains with PDF semantics out
  of the box. webpki is the most used and best-tested RFC 5280 core, and
  its usage and time hooks let sheaf supply the PDF policy.
- **Recommendation:** **build on rustls-webpki, behind a trait;** sheaf
  adds the EKU policy, the signing time, revocation from DSS and CMS, and
  the legacy algorithms. Medium. Re-check synta at kickoff.

#### 48. PKCS#12 files

- **Need:** open .p12 and .pfx files exported by Windows, macOS, Java and
  OpenSSL: legacy PBE (SHA-1 with 3DES or RC2-40) and PBES2 (PBKDF2 with
  AES). Find the key and its chain.
- **Where:** M12 slice 2 ("sign with PKCS#12 files", `ROADMAP.md:397`).
- **Candidates:**
  - **[p12-keystore](https://crates.io/crates/p12-keystore)** 0.4.1
    (2026-10-13; MIT/Apache-2.0; ancwrd1 alone; 17 releases since
    2024-03; 9.6M downloads; 25★; pure Rust on RustCrypto; no `unsafe`).
    Reads and writes "modern and legacy encryption schemes"; no MD5-based
    schemes or unencrypted stores (README). **It depends on
    pre-releases** (cms 0.3.0-pre.1, and pkcs12 0.2.0-pre.0, which pins
    cms `=0.3.0-pre.1`) and on x509-parser 0.18.
  - [pkcs12](https://crates.io/crates/pkcs12) (RustCrypto) 0.1.0 and
    0.2.0-pre.0: types and the PKCS#12 KDF only.
  - [p12](https://crates.io/crates/p12) 0.6.3: last release 2022-02-18.
    Stale. And openssl's `Pkcs12`.
- **Fit:** p12-keystore covers it, but can't share a build with cms
  0.3.0-rc.0 today (entry 44).
- **Recommendation:** **adopt p12-keystore once it moves to the published
  cms 0.3.** Until then, M12 pins the pre-release set it needs. Medium.

#### 49. PKCS#11 tokens

- **Need:** load a token's module, list slots and certificates, log in
  with a PIN, sign (C_Sign) and later decrypt (C_Decrypt).
- **Where:** M12 slice 4 ("PKCS#11 tokens", `ROADMAP.md:398`).
- **Candidates:** **[cryptoki](https://crates.io/crates/cryptoki)**
  0.12.1 (2026-09-16; Apache-2.0; the Parsec team: wiktor-k, ionut-arm,
  hug-dev and the org; 3.3M downloads; 120★). It released 0.10.1, 0.11.1
  and 0.12.1 the same day, so older lines get fixes. **FFI** through
  libloading: the vendor's module loads at run time (119 `unsafe` sites).
  PINs held with secrecy. [pkcs11](https://crates.io/crates/pkcs11)
  0.5.0: last release 2020, stale.
- **Fit:** covers the API. Module discovery is sheaf's: p11-kit's proxy
  on Linux, the vendor's DLL or OpenSC on Windows and macOS. Reaching
  host tokens from inside Flatpak (p11-kit server forwarding) is
  unverified.
- **Recommendation:** **adopt cryptoki.** High.

#### 50. Windows certificate store and CNG signing

- **Need:** list the user's `MY` certificates with private keys, filter
  by KeyUsage and EKU, offer a picker; sign a hash through NCrypt
  (smart cards behind CNG minidrivers, with Windows asking for the PIN);
  read ROOT and other stores for verification.
- **Where:** M12 slice 4 ("the Windows certificate store",
  `ROADMAP.md:397`).
- **Candidates:** **[windows](https://crates.io/crates/windows)** 0.62.2
  (2025-10-06; MIT OR Apache-2.0; Microsoft's kennykerr; 375M downloads):
  FFI bindings; sheaf-instance uses 0.61. CertOpenStore,
  CryptAcquireCertificatePrivateKey, NCryptSignHash, NCryptDecrypt,
  CryptUIDlgSelectCertificateFromStore.
  [schannel](https://crates.io/crates/schannel) 0.1.30 (2026-10-16; MIT;
  sfackler and steffengy; 340M downloads; windows-sys 0.61):
  `CertStore::open_current_user`, `CertContext::private_key().acquire()`
  gives an `NcryptKey` handle, but **no signing call**: it's for TLS.
- **Recommendation:** **build on the windows crate** (on GPUI's 0.62, as
  entry 7 says); schannel optional for listing. Medium-high.

#### 51. macOS keychain (missing from the plan)

- **Need:** identities in the login and system keychains, smart cards
  through CryptoTokenKit, signing with `SecKeyCreateSignature`, trust
  settings for verification.
- **Where:** M12 slice 4. **Not in `ROADMAP.md:397-398`,** though macOS
  is a target platform. D8.
- **Candidates:** **[security-framework](https://crates.io/crates/security-framework)**
  3.7.0 (2026-02-20; MIT OR Apache-2.0; kornelski and djc; 416M
  downloads; 306★; FFI; already in the lockfile). Identity search with
  `ItemSearchOptions` (`src/item.rs:62-66`);
  `SecKey::create_signature(Algorithm, input)` and `verify_signature`
  (`src/key.rs:228-250`).
- **Recommendation:** **adopt it, and add the keychain to the plan.**
  High.

#### 52. RFC 3161 timestamps

- **Need:** build a TimeStampReq, POST it (`application/timestamp-query`),
  parse the TimeStampResp, check its token (the TSA's chain with
  id-kp-timeStamping, the messageImprint, the nonce); embed the token as
  a signature timestamp (PAdES B-T) or a `/DocTimeStamp` (ETSI.RFC3161,
  B-LTA); verify existing tokens (the spike matched 53 imprints).
- **Where:** M12 slice 5 ("RFC 3161 timestamps; LTV (embed revocation
  info)", `ROADMAP.md:401`). D13.
- **Candidates:**
  - [x509-tsp](https://crates.io/crates/x509-tsp) 0.1.0 (2023-06-26;
    Apache-2.0 OR MIT; RustCrypto): types only, on the old line (der
    0.7, cms 0.2). 0.2.0-pre on cms 0.3.0-rc.0 is on master,
    unpublished.
  - **[tsp-ltv](https://crates.io/crates/tsp-ltv)** 0.6.0 (2026-10-16;
    BSD-2-Clause; Kushal Das alone; created 2026-03; 228k downloads; 2★;
    no `unsafe`): a TSA client and pool, OCSP and CRL clients, chain
    building, trust stores. On the RustCrypto 0.2 line, with reqwest
    0.12, tokio and rustls; async, with blocking wrappers.
  - cryptographic-message-syntax 0.28 has a TSP client (MPL-2.0; entry
    44).
  - **HTTP:** sheaf-app already links hyper 1.12, tokio and rustls 0.23
    (through wasmtime-wasi-http). The installer plan picked ureq
    (`todos/installer.md:81`). TSAs, OCSP and CRLs are mostly plain HTTP.
- **Recommendation:** **build on x509-tsp 0.2 and cms 0.3** once
  published, with a small synchronous, bounded HTTP client. Medium. Use
  tsp-ltv as prior art and a test oracle; adopt it only if it moves to
  the 0.3 line and lets callers supply the transport (kickoff).

#### 53. OCSP and CRL: fetch, parse, check

- **Need:** parse OCSP responses and CRLs from the DSS, from Adobe's
  `adbe-revocationInfoArchival` signed attribute (144 corpus signatures
  carry one) and from online fetches; build OCSP requests and verify
  responders; fetch from AIA and CRL distribution points under caps (CRLs
  can be megabytes).
- **Where:** M12 slices 3 and 5 ("LTV (embed revocation info)",
  `ROADMAP.md:401`).
- **Candidates:** [x509-ocsp](https://crates.io/crates/x509-ocsp) 0.2.1
  (2024-01-08; Apache-2.0 OR MIT; tarcieri): types and a builder on der
  0.7; 0.3.0-pre on master, unpublished. x509-cert's
  `crl::CertificateList`. rustls-webpki's CRL checks (no OCSP).
  [rasn-ocsp](https://crates.io/crates/rasn-ocsp) 0.29.0. tsp-ltv (entry
  52).
- **Recommendation:** **build on x509-ocsp 0.3 and x509-cert's types**,
  with sheaf's HTTP client and caps; tsp-ltv as prior art. Medium.

#### 54. LTV (DSS and VRI) and PAdES levels

- **Need:** write a `/DSS` with `/Certs`, `/OCSPs` and `/CRLs`,
  optionally `/VRI` keyed by the uppercase hex SHA-1 of the signature's
  `/Contents`, in an incremental update after signing. PAdES levels: B-B
  (signingCertificateV2, no signingTime), B-T (a signature timestamp),
  B-LT (DSS), B-LTA (a document timestamp after the DSS). Read the DSS
  and Adobe's archive attribute for validation. The verify panel's
  "changes since signing", by comparing revisions.
- **Where:** M12 slices 3, 5 and 6.
- **Today's plan:** "Signing appends: it must never rewrite earlier
  revisions, and M10.5's Append (slice 10) provides that"
  (`ROADMAP.md:407-409`).
- **Candidates (none fits; all checked):**
  - [underskrift](https://crates.io/crates/underskrift) 0.1.4
    (2026-06-01; BSD-2-Clause; Kushal Das; 3.8k downloads; 13★): signs
    and verifies B-B through B-LTA, with remote (prepared hash) signing
    and policy hooks. On lopdf 0.39 and the RustCrypto 0.2 line, bytes in
    and bytes out, 24k lines. Its `core/revision.rs` sorts later
    revisions into safe and unsafe changes: prior art for the panel.
  - [pades](https://crates.io/crates/pades) 0.1.4: **first published
    2026-10-23**, 8 downloads, Apache-2.0; its own SHA-2, RSA, DER and CMS
    (3k lines). Don't use it.
  - [pdf_signing](https://crates.io/crates/pdf_signing) 0.3.0
    (2024-11-08; MIT OR Apache-2.0): lopdf 0.34,
    cryptographic-message-syntax, PNG-only appearance, "work in
    progress".
  - [pdf_signer](https://crates.io/crates/pdf_signer) 0.3.2:
    **GPL-3.0-or-later, out**.
  - [pdf_oxide](https://crates.io/crates/pdf_oxide) 0.3.78 (its
    `signatures/` has PAdES and a TSA client) and
    [oxidize-pdf](https://crates.io/crates/oxidize-pdf) 5.5.1: whole PDF
    stacks with their own parsers, so they can't sit on sheaf's Append
    and serializer.
  - Prior art: [pyHanko](https://pypi.org/project/pyHanko/) 0.38.0
    (2026-10-17; MIT; 791★): signing and validation separate;
    `DocumentSecurityStore.register_vri` and PAdES-LT and LTA profiles
    ([dss](https://pyhanko.readthedocs.io/en/0.15.1/api-docs/pyhanko.sign.validation.dss.html),
    [validation](https://pyhanko.readthedocs.io/en/latest/api-docs/pyhanko.sign.validation.html));
    `ades_lta_validation`, after ETSI EN 319 102-1 §5.6.3. EU DSS (Java,
    LGPL): reference only.
- **Recommendation:** **build in-house**, on M10.5's Append and the
  formats crates. High that no crate fits. pyHanko, already the corpus's
  oracle, checks interop; underskrift can be a second oracle in tests.

#### 55. Trust anchors for verification

- **Need:** what to trust: certificates the user imports; the OS stores
  (Windows ROOT, macOS trust settings); the Adobe Approved Trust List;
  the EU Trusted Lists. Linux has no store for document-signing roots,
  and webpki-roots is a TLS list.
- **Where:** M12 slice 3; the roadmap's "validation status" implies it.
  D13.
- **Candidates:** the windows crate and security-framework read the OS
  stores (entries 50–51). No Rust crate verifies the EU list's XAdES
  signature maturely; Kushal Das's bergshamra and uppsala are new and
  weren't checked. Whether third parties may use the Adobe list is
  unverified.
- **Recommendation:** **undecided;** decide at kickoff. A reasonable
  start: user-trusted certificates plus the OS stores, the EU list later.
  Low.

#### 56. A script font for typed signatures (not in the plans)

- **Need:** "Fill & Sign: typed, drawn, or image signature and initials"
  (`ROADMAP.md:396`). A typed signature is drawn in a handwriting-style
  font, as Acrobat does; sheaf's needs are unverified.
- **Where:** M12 slice 1. D13.
- **Candidates:** an OFL or Apache script font, embedded through entry
  17's subsetting. Not chosen; it needs a license check.
- **Recommendation:** **undecided.** Pick the font at kickoff; the
  embedding path is M10 slice 7's. Low.

#### 57. Certificate encryption (`/Adobe.PubSec`), after M12

- **Need:** decrypt: find the user's recipient in `/Recipients` (CMS
  EnvelopedData), recover the 20-byte seed by RSA key transport (PKCS#1
  v1.5 or OAEP) or ECDH, decrypt the envelope (AES, 3DES or RC2), derive
  the file key with SHA-1 (s4) or SHA-256 (AES-256). Encrypt for chosen
  recipients.
- **Where:** after M12; M11 slice 1's refusal.
- **Today's plan:** "Certificate encryption (`/Adobe.PubSec`) comes after
  M12 … PDFium only opens the standard (password) handler"
  (`m11.md:69-72`); "It needs the certificate stores built here, and
  sheaf has to decrypt these files itself" (`ROADMAP.md:411-415`);
  "Certificate-encrypted files get a clear refusal" (`m11.md:425`).
- **Corpus:** 12 such files in `signatures/pyhanko/pdf/`, 9 with
  adbe.pkcs7.s5 and 3 with s4 (`minimal-pubkey-aes256`, `-rc4`,
  `-rc4-envelope`, `-unknown-envelope-alg`, `pubkey-3des-test`,
  `pubkey-rc2-test`, `pubkey-ecc-test` and others). The keys that open
  them are in pyHanko's repository (MIT).
- **Candidates:** cms 0.3 (EnvelopedData, its recipient types and
  builders; `src/enveloped_data.rs:33-218`, `builder/kari.rs`);
  [des](https://crates.io/crates/des) 0.9.0 and
  [rc2](https://crates.io/crates/rc2) 0.9.0 (2026-04-10, RustCrypto),
  plus aes, cbc, sha1 and sha2; p256 and p384 with ansi-x963-kdf for
  ECDH. Key unwrap: the rsa crate (Marvin is about exactly this kind of
  PKCS#1 v1.5 decryption; local use only), aws-lc-rs'
  `Pkcs1PrivateDecryptingKey` (constant-time, a C build), or the key
  stores' own decrypt (NCryptDecrypt, `SecKeyCreateDecryptedData`,
  C_Decrypt). Prior art: pyHanko's `PubKeySecurityHandler`
  ([docs](https://docs.pyhanko.eu/en/latest/api-docs/pyhanko.pdf_utils.crypt.html)):
  the s3, s4 and s5 subfilters, RSA PKCS#1 v1.5 or OAEP key transport,
  ECDH ephemeral-static with the X9.63 KDF, AES, AES-GCM and RC4 crypt
  filters.
- **Recommendation:** **build on cms and RustCrypto,** with the key
  operation behind M12's key-store trait, which gains a decrypt. Medium.
  M11 slice 1's refusal can be tested on these 12 files now.

### M13: Content editing

#### 58. Glyph coverage, and editing in an existing embedded font

- **Need:** decide whether an edited string can be drawn in the run's own
  font (does the program have the glyph, and what code does it get?), or
  give "a clear warning when a glyph is missing"; match a substitute when
  the embedded font is a subset.
- **Where:** M13 "text edit within a line"; `ROADMAP.md:418-420`: "font
  matching with substitution when the embedded font is subset, and a
  clear warning when a glyph is missing".
- **Candidates:** skrifa (entry 25): charmap, glyph names,
  `outline_glyphs().get(gid)`.
- **Fit:** most embedded fonts are subsets made by another tool, which
  renumbered their glyphs and usually dropped GSUB/GPOS. Glyphs can't be
  merged into them: new text that needs new glyphs goes in a new font
  resource. A glyph present but blank can be a space, or a glyph another
  subsetter emptied while keeping IDs; telling them apart takes
  heuristics (an advance but no contours, against a mapped space).
- **Recommendation:** **build on skrifa.** High.

#### 59. Shaping and bidi reordering

- **Need:** shape runs (ligatures, kerning, marks, complex scripts) and
  reorder right-to-left text for M13 editing, and for fields "later"
  (`m10.md:2052`).
- **Where:** M13; M10 "Not in M10" (`m10.md:2052-2053`).
- **Today's plan:** out of M10 with a notice (`m10.md:614-615`); nothing
  for M13.
- **Candidates:**
  - **[harfrust](https://crates.io/crates/harfrust) 0.15.0**
    (2026-10-22; MIT; the harfbuzz org; 32 versions, 23 releases in 12
    months; 14.4M downloads). On read-fonts, the same stack as skrifa;
    "Matches HarfBuzz v15.0.0"; "less than 25% slower than HarfBuzz";
    `forbid(unsafe)` on the lib. In the Linux app already (0.5.2, through
    cosmic-text). "Malformed fonts will cause an error", where HarfBuzz
    falls back.
  - **[rustybuzz](https://github.com/harfbuzz/rustybuzz) 0.20.1:**
    archived, "not developed further, unmaintained … switch to HarfRust".
    In every sheaf-app build through usvg, but not to build on.
  - HarfBuzz through harfbuzz-sys 0.8.0: C++ FFI; no reason to take C++
    over harfrust.
  - [swash](https://crates.io/crates/swash) 0.2.10 (Apache-2.0 OR MIT):
    a simpler shaper than HarfBuzz's, mainly a scaler. Not for PDF
    output.
  - unicode-bidi 0.3.18: UAX #9, already compiled (entry 19). ICU4X has
    no full-UBA crate.
- **Fit:** harfrust gives glyph IDs and advances in font units, what a
  PDF `TJ` needs. It only shapes SFNT fonts with layout tables, so
  complex-script editing needs a full installed or bundled font, never an
  existing subset.
- **Recommendation:** **adopt harfrust and unicode-bidi** when M13 (or
  RTL fields) starts. High.

#### 60. Block reflow and line breaking

- **Need:** re-break a text block after an edit: UAX #14 break
  opportunities, fit to the block's width in the PDF fonts, PDF text
  state kept (`Tc`, `Tw`, `Tz`, `TL`).
- **Where:** M13 "block reflow" (`ROADMAP.md:428`).
- **Today's plan:** none; `wrap_with` breaks only at spaces.
- **Candidates:**
  - **[icu_segmenter](https://crates.io/crates/icu_segmenter) 2.4.0**
    (2026-10-22; Unicode-3.0; ICU4X; 23.6M downloads; 7 releases in 12
    months): line, word, grapheme and sentence segmentation. The `lstm`
    and dictionary models (Thai, Lao, Khmer, Burmese) are optional. 4.47
    MB packaged; data through `icu_segmenter_data`. boa depends on it
    inside the wasm guest, so it isn't shared with the host.
  - **[unicode-linebreak](https://crates.io/crates/unicode-linebreak)
    0.1.5** (2023-07-25; Apache-2.0; tiny, no `unsafe`, Unicode 15.0; not
    pushed since 2023-07, so lagging). In the Linux app through
    cosmic-text.
  - [parley](https://crates.io/crates/parley) 0.12.0 /
    [fontique](https://crates.io/crates/fontique) 0.12.0 (2026-10-09;
    Apache-2.0 OR MIT; linebender): rich-text layout over its own font
    collection.
  - [cosmic-text](https://crates.io/crates/cosmic-text) 0.19.0
    (2026-04-22; MIT OR Apache-2.0; System76): GPUI's Linux text system.
- **Fit:** parley and cosmic-text lay out UI text over fonts they load
  (SFNT, through fontique or fontdb). They can't take an embedded Type 1,
  a CFF-only CID font or a Type3 font, and they don't model PDF text
  state. The layout has to be sheaf's, over the `Font` trait, with a
  segmenter for break opportunities.
- **Recommendation:** **build in-house on icu_segmenter** (maintained,
  current Unicode; the line segmenter without LSTM). unicode-linebreak is
  the lighter alternative. Not parley or cosmic-text as the engine.
  Medium.

#### 61. Font matching, installed fonts and fallback

- **Need:** find an installed font to substitute for a subset, or for a
  character no bundled font covers; map PDF names and descriptor flags to
  candidates; respect OS/2 `fsType` embedding permissions.
- **Where:** M13 (`ROADMAP.md:418-420`); M10 "Installed fonts … (found
  with `fontdb`, embedded where the font allows): later"
  (`m10.md:2049-2051`); M14 (`m10.5-edit-api.md:3663-3668`).
- **Today's plan:** fontdb named for later; matching not designed.
- **Candidates:**
  - **[fontdb](https://crates.io/crates/fontdb) 0.24.0** (2026-07-29;
    MIT; 40M downloads; one release in 12 months): pure Rust; scans font
    directories and parses fontconfig's config files (not
    libfontconfig); its `memmap` feature uses `unsafe` mmap. **0.23 is
    compiled on every platform already.**
  - [fontique](https://crates.io/crates/fontique) 0.12.0 (Apache-2.0 OR
    MIT; linebender; 7 releases in 12 months): native enumeration through
    DirectWrite, CoreText and fontconfig (`fontconfig-dlopen`), on
    read-fonts 0.41. More faithful to what the OS calls installed
    (per-user fonts, activation).
  - [font-kit](https://crates.io/crates/font-kit) 0.14.3 (2025-05-26; MIT
    OR Apache-2.0; servo): no release in 12 months; FreeType and native
    loaders. GPUI's Linux build carries a Zed fork.
- **Fit:** these enumerate and give file bytes; matching is sheaf's.
  pdf.js's font alias tables (Apache-2.0, e.g. ArialMT → Helvetica) and
  PDFium's `CFX_FontMapper` (BSD-3) are the references, with
  attribution. Whether fontdb finds Windows' per-user fonts
  (`%LOCALAPPDATA%\Microsoft\Windows\Fonts`) is unverified.
- **Recommendation:** **build matching in-house; adopt fontdb** for
  enumeration. Switch to fontique if fontdb misses user-installed fonts
  on Windows or macOS. Medium.

### M14: Document processing

#### 62. Re-subsetting fonts already in the file (optimize)

- **Need:** shrink embedded programs to the glyphs content still uses,
  without changing the codes in content streams.
- **Where:** M14 optimize (`ROADMAP.md:436-437`, "font subsetting");
  M11's leak (entry 28).
- **Candidates:** subsetter (SFNT only, renumbers); skera (`glyf` only,
  RETAIN_GIDS); allsorts (SFNT, renumbers); hb-subset (C++, SFNT). None
  takes bare CFF (`FontFile3 /Type1C`, `/CIDFontType0C`) or Type 1
  (`FontFile`) as input, and renumbering would mean rewriting every
  content stream that uses the font, plus `/W`, `/Widths` and
  `/CIDToGIDMap`.
- **Fit:** blanking in place (entry 28) keeps the codes and gets most of
  the size back. Glyph closure (composites, `seac` accents in Type 1 and
  CFF) has to be followed. Inputs are untrusted: blanking is bounded,
  and refuses malformed programs, leaving them as they are.
- **Recommendation:** **build in-house on read-fonts.** No crate covers
  it. Medium.

#### 63. PDF/A and preflight font checks; embedding the standard 14

- **Need:** check fonts are embedded, `/Widths` agree with the program's
  advances, and `CIDSet`/`CharSet` are present for PDF/A-1 subsets. With
  `Profile.fonts: EmbedAll`, embed substitutes for every standard 14
  font sheaf writes, ZapfDingbats check boxes among them.
- **Where:** M14 PDF/A and preflight-lite (`ROADMAP.md:456`);
  `m10.5-edit-api.md:3663-3668` ("`Profile.fonts: Std14Ok | EmbedAll` …
  Metric-compatible substitutes (Liberation) sit behind `Font`"). D10.
- **Candidates:** skrifa (advances); Liberation (Helvetica, Times and
  Courier only); **Foxit's PDFium substitutes,** FoxitSymbol.pfb and
  FoxitDingbats.pfb (Type 1, 16.7 KB and 29.5 KB; BSD-3, PDFium
  Authors), which pdf.js ships in
  [external/standard_fonts](https://github.com/mozilla/pdf.js/tree/master/external/standard_fonts)
  with Liberation Sans.
- **Fit:** **Liberation has no Symbol or ZapfDingbats.** The Foxit pair
  is what PDFium, and so sheaf's renderer, draws for them. Embedding a
  Type 1 is fine for PDF/A.
- **Recommendation:** **adopt** Foxit Symbol and Dingbats for EmbedAll;
  **build** the checks on skrifa. Medium.

#### 64. Image downsampling

- **Need:** resample page images to a target ppi in gray, RGB, CMYK and
  16-bit, then re-encode.
- **Where:** `ROADMAP.md:436-437` ("image downsampling and
  recompression").
- **Today:** sheaf resizes with `image::imageops::resize` (`clean.rs:251`
  and others, `scan.rs:135`).
- **Candidates:** image's `resize` (pure Rust, safe, slow; figures
  unverified); [`fast_image_resize`](https://crates.io/crates/fast_image_resize)
  6.1.0 (2026-07-21; MIT OR Apache-2.0; Cykooz alone; 22M downloads;
  SIMD with about 680 `unsafe` in `src/`; U8 to U8x4, U16 and F32 pixels,
  so CMYK works as U8x4; a wasm32 SIMD path; rayon optional);
  [`pic-scale`](https://crates.io/crates/pic-scale) 0.7.12 (2026-09-10;
  BSD-3-Clause OR Apache-2.0; awxkee; used by hayro); hayro-jpeg2000's
  `target_resolution`, which decodes JPX at reduced size.
- **Recommendation:** **adopt fast_image_resize** for M14's new paths,
  after measuring it against image's resize on corpus scans. Leave
  cleanup's existing calls alone (byte-identical rule). Medium.

#### 65. Recompression and lossless Flate

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

#### 66. Object-stream packing

- **Need:** pack eligible objects into object streams on Rewrite (and
  Append), with ISO 32000-1 7.5.7's exclusions, encrypting each
  container once.
- **Where:** `ROADMAP.md:436-437`; M10.5 §20
  (`m10.5-edit-api.md:3659-3664`); decisions (`m10.5-decisions.md:530`,
  `:1957`).
- **Today's plan:** "Packing in Rewrite and Append, with
  `packable(ObjKey, &SymObject, gen)` encoding ISO 32000-1 7.5.7's
  exclusions … Member strings are never encrypted individually; the
  container is" (`m10.5-edit-api.md:3659-3664`).
- **Candidates:** lopdf 0.46's `ObjectStreamBuilder` and `SaveOptions`
  (`use_object_streams`), about 350 lines, only for lopdf's own
  `Document` writer; pdf-writer has no object-stream builder that was
  found; qpdf (`--object-streams=generate`) is C++.
- **Fit:** the packing is easy (a header of number–offset pairs, the
  objects, Flate). The hard parts are sheaf's: the exclusions,
  encryption, which members a signature needs left in place, the
  xref-stream base. Those already live in the serializer.
- **Recommendation:** **build in-house** in `serialize/`, with lopdf's
  `object_stream.rs` as a reference. Check with `qpdf --check`, PDFium
  and pdf.js reading the result back, and the corpus. High.

#### 67. Linearization (Fast Web View) and B3

- **Need:** write linearized files as an option on a full rewrite (page
  1's objects first, the linearization dictionary, hint tables); report
  linearization truthfully after an Append (B3).
- **Where:** `ROADMAP.md:438-454`; B3 (`backlog.md:90-121`).
- **Today's plan:** "A writer option, beside object streams … Not an edit
  and not a transform plugin … Checked with `qpdf
  --check-linearization`" (`ROADMAP.md:442-454`).
- **Candidates:** no Rust crate writes linearized files; lopdf 0.46's
  `SaveOptions.linearize` is only a flag (nothing in `src/` acts on it).
  qpdf (`--linearize`; C++, Apache-2.0) does; locally qpdf is 12.3.2,
  the latest 12.4.2 (2026-09-27).
- **Fit:** qpdf's `QPDF_linearization.cc` and `QPDFWriter` (Apache-2.0,
  readable), with ISO 32000-1 Annex F, are the reference.
- **Recommendation:** **build in-house** on the serializer, as planned;
  test with `qpdf --check-linearization` on Linux CI, where qpdf is a
  test tool already. B3's fix is sheaf's own (`/L` against the file
  length). High that no crate exists; medium on size (hint tables are
  the bulk).

#### 68. PDF/A conversion and validation; preflight-lite

- **Need:** convert to PDF/A (embedded fonts, an output intent with an
  ICC profile, XMP agreeing with Info, no forbidden features: JavaScript,
  encryption, LZW, transparency in PDF/A-1); validate a file's PDF/A
  claim; preflight-lite checks (font embedding, color spaces,
  transparency).
- **Where:** `ROADMAP.md:455-456`; M10.5 §20
  (`m10.5-edit-api.md:3669-3670`, fonts at `:3665-3668`); M10.5's D76
  (`m10.5-decisions.md:1785-1791`); F9 (`backlog.md:665-686`). D3, D14.
- **Today's plan:** "PDF/A convert and validate ❓" and "Preflight-lite …
  ❓" (`ROADMAP.md:455-456`), how far to go decided at kickoff
  (`:460-461`). M10.5 adds only the Note.
- **Candidates:**
  - **veraPDF** ([library](https://github.com/veraPDF/veraPDF-library),
    [apps](https://github.com/veraPDF/veraPDF-apps)): Java, GPLv3+ or
    MPLv2+ (library README). The latest on Flathub is 1.30.1, listed "6
    months ago" ([Flathub](https://flathub.org/apps/org.verapdf.veraPDF));
    its changelog has PDF/UA-2 and WTPDF fixes and adds Java 25; 1.31.x
    tags are development builds. The CLI takes `--flavour` with
    `1a`…`4f`, `ua1`, `ua2`, `wt1r` and `wt1a`, and `--format json`, and
    `--recurse`s into directories
    ([CLI docs](https://docs.verapdf.org/cli/validation/)); exit codes
    aren't documented. Docker [`verapdf/cli`](https://hub.docker.com/r/verapdf/cli):
    the newest tag a search showed was v1.28.2, possibly stale. No Java
    on this machine today.
  - **pdf_oxide 0.3.78** `src/compliance/`: `pdf_a.rs` 7.5 KB,
    `validators.rs` 24 KB, a 93 KB PDF/X validator; its converter falls
    back to a built-in sRGB profile stored as
    `srgb_profile_placeholder.bin`. Shallow next to veraPDF's rule
    profiles.
  - **oxidize-pdf 5.5.1:** its own claims list says PDF/A validation is
    "Basic functionality … not independent certification", and makes "no
    … PDF/UA compliance" claim (`docs/CLAIMS.md:40,51`). GPL, then AGPL,
    then MIT.
  - **The Arlington PDF Model**
    ([pdf-association/arlington-pdf-model](https://github.com/pdf-association/arlington-pdf-model)):
    Apache-2.0, pushed 2026-10-21. TSV files describing every PDF 2.0
    object, with version predicates. TestGrammar (C++) is its proof of
    concept; veraPDF publishes an Arlington service (entry 94).
  - **ICC data:** [Compact-ICC-Profiles](https://github.com/saucecontrol/Compact-ICC-Profiles)
    is CC0-1.0 ("released to the public domain"). krilla ships its
    `sRGB-v4.icc`, `sRGB-v2-magic.icc`, `sGrey-v4.icc` and
    `sGrey-v2-magic.icc` (`crates/krilla/icc/`). Which one each PDF/A
    part needs (v2 or v4) is unverified. (The images section would
    generate the profile with moxcms instead: D14.)
  - **Data:** `../pdf-corpus/conformance/` has 3,524 files from the bfo
    and veraPDF suites, among others; its manifest gives the expected
    result and clause per file (`expect`, `profile`, `clause`): ground
    truth for any validator.
- **Fit:** no Rust crate validates foreign files near veraPDF's depth.
  Conversion is sheaf's own, on its emitter (fonts: entry 63; images:
  entries 33–38, 65; XMP: entry 41). Bundling veraPDF in the app would
  mean shipping a JRE: the MPL option allows it, but it's heavy and
  foreign to sheaf's stack.
- **Recommendation:** **build in-house** for conversion and for the rules
  covering what sheaf writes, modeled on krilla's clause tables. Use
  **veraPDF as a test tool, never linked**: a corpus tier over
  `conformance/` comparing sheaf's checks with veraPDF's JSON, and a tier
  running veraPDF on sheaf's converted output. Medium. User-facing
  validation of other tools' files: M14's kickoff decides; if wanted,
  veraPDF as an optional external tool or plugin, not a rule engine in
  Rust.

#### 69. Structure trees and tags (M14 and M17)

- **Need:** model `/StructTreeRoot`, `/ParentTree` (a number tree),
  `StructElem` kids, MCIDs and OBJRs; emit them from the model; tag what
  sheaf writes or mark it as an artifact; keep a tagged file tagged
  across edits.
- **Where:** M10.5 §20 "Structure" (`m10.5-edit-api.md:3671-3673`) and
  M10.5's D75 (`m10.5-decisions.md:1769-1783`); `ROADMAP.md:576-602` (M17); M10's
  copies "untagged until M14" (`m10.md:1893-1901`).
- **Today's plan:** "`Key::StructElem`, `/ParentTree` emitted from the
  model, and `PlaceTag::Struct(elem)` for flatten"
  (`m10.5-edit-api.md:3671-3673`).
- **Candidates:** no crate edits an existing structure tree. krilla tags
  new documents only. pdf-writer has typed writers (`structure.rs`).
  pdf_oxide has a structure *reader* (`src/structure/parser.rs`,
  `traversal.rs`). pdf.js reads structure trees for accessibility
  (`src/core/struct_tree.js`, Apache-2.0), and PDFBox has
  `PDStructureTreeRoot` (Apache-2.0); both are reading models.
- **Fit:** the tree is part of sheaf's model and history (keys, Puts,
  removal), so no outside type can own it.
- **Recommendation:** **build in-house.** Use krilla's `PDF_UA1.md` rules
  ("krilla never includes artifacts in the structure tree", role maps for
  non-standard types) and pdf-writer's key names as references, and
  veraPDF `ua1`/`ua2` plus Arlington in tests. High.

#### 70. Compare: visual (pixel) diff

- **Need:** pages rendered at the same dpi, compared with tolerance for
  anti-aliasing, the changed regions boxed.
- **Where:** `ROADMAP.md:434-435`.
- **Today's plan:** none.
- **Candidates:**
  - [`dssim`](https://crates.io/crates/dssim) and
    [`dssim-core`](https://crates.io/crates/dssim-core) 3.5.1
    (2026-08-29): **AGPL-3.0. Out.**
  - [`image-compare`](https://crates.io/crates/image-compare) 0.5.0
    (2025-08-18; MIT; ChrisRega alone; 2.1M downloads; no `unsafe`; on
    image, itertools and rayon): SSIM, RMS, a "hybrid" score, difference
    images.
  - [`blazediff`](https://crates.io/crates/blazediff) 6.0.1 (2026-08-27;
    19 releases since 2026-01; MIT; teimurjan; 533k downloads in 90
    days; 352 stars): pixelmatch-compatible YIQ delta and anti-alias
    detection; SIMD (63 `unsafe`); `default-features = false` drops its
    command line and its PNG and JPEG code.
  - [`dify`](https://crates.io/crates/dify) 0.8.0 (MIT): a command-line
    tool. [`pixelmatch`](https://crates.io/crates/pixelmatch) 0.1.0
    (2021): stale. pixelmatch's own algorithm (mapbox, ISC) is about 200
    lines and easy to port.
- **Fit:** the kernel is the easy part. Matching pages across inserted
  or deleted pages, and grouping changed pixels into boxes (imageproc's
  connected components, already used), are sheaf's either way.
- **Recommendation:** **build on PDFium renders and imageproc,** with
  blazediff behind a small trait or a credited port of pixelmatch's
  algorithm. image-compare only if a single similarity score is wanted.
  Medium.

#### 71. Compare: text diff

Both the images and core sections; the images section's detail stands,
and core's benchmark is the check.
- **Need:** a word-level diff of two documents' text, with positions so
  changes can be drawn; page matching between versions; bounded at
  7,000 pages.
- **Where:** `ROADMAP.md:434-435` ("Compare documents: text diff and
  visual (pixel) diff, with a side-by-side report"; no crate named).
- **Candidates:**
  - [`similar`](https://crates.io/crates/similar) 3.2.0 (2026-08-17;
    Apache-2.0; mitsuhiko; 228M downloads; no `unsafe`): word, character
    and grapheme tokenizers; Myers, patience and LCS; **deadline
    variants** ("algorithms periodically check it and may fall back",
    `algorithms/mod.rs:51-92`); wasm support (`wasm32_web_time`).
  - [`imara-diff`](https://crates.io/crates/imara-diff) 0.2.0
    (2025-06-14; Apache-2.0; 35M downloads): faster, lower-level
    (interned tokens), 10 `unsafe`; used by gitoxide (unverified).
  - [`dissimilar`](https://crates.io/crates/dissimilar) 1.0.11
    (2026-03-15; Apache-2.0; dtolnay): character-level diff-match-patch
    with semantic cleanup. [`diff`](https://crates.io/crates/diff)
    0.1.13 (2022): stale.
- **Fit:** similar's deadline keeps the work bounded, and the same crate
  can match pages by diffing a sequence of page fingerprints. Page
  matching and the report are sheaf's.
- **Recommendation:** **adopt similar;** imara-diff if 7,000-page runs
  are too slow, decided by a benchmark at M14's kickoff. High (images);
  medium (core, pending the benchmark).

#### 72. Compare: the side-by-side report

- **Need:** a PDF report with both versions' pages side by side and the
  changes highlighted.
- **Candidates:** none needed: sheaf has its own content writer, the
  image-XObject code in `sheaf-doc/src/images.rs`, the standard 14
  fonts, and the comment-summary report as a precedent.
- **Recommendation:** **build in-house.** High.

### M15: Automation

#### 73. Batch actions, `sheaf batch`, and the optional Rhai front end

- **Need:** a versioned step schema over `Op`, a host runner, a CLI, and
  maybe a macro language for non-programmers.
- **Where:** `ROADMAP.md:462-479`; M10.5 §20 "M15 (batch)"
  (`m10.5-edit-api.md:3681-3686`); README's open question
  (`README.md:365-366`) and its feature row (`:304`).
- **Today's plan:** "Built on the M6 plugin host API, which settles the
  README's Rhai vs Lua question: plugins first, with an optional Rhai
  front end for quick macros if users want one" (`ROADMAP.md:466-468`).
- **Candidates:** [rhai](https://crates.io/crates/rhai) 1.27.0
  (2026-10-21; MIT OR Apache-2.0; 106 releases since 2016; 13.4M
  downloads; pure Rust; builds for wasm). `Engine::set_max_operations`
  stops runaway scripts
  ([Rhai book](https://rhai.rs/book/safety/max-operations.html)), and
  "Rhai is designed to not bring down the host system"
  ([Safety](https://rhai.rs/book/safety/)).
  [mlua](https://crates.io/crates/mlua) 0.12.2 (2026-10-03; MIT): Lua or
  LuaJIT in C through FFI.
- **Fit:** a step list is serde and schemars data, like `Op`, and needs
  no crate. Rhai fits a macro front end that calls the same commands.
  Macros are the user's own (or the firm's), not something a PDF
  carries, so running them in process with operation limits is
  defensible. If macros are shared across a firm, the boa approach (a
  wasm guest) applies too.
- **Recommendation:** **build** the runner and step schema in-house. For
  macros, **undecided:** if users ask for macros beyond plugins (the
  roadmap already says "if users want one"), adopt Rhai. Medium.

#### 74. Watched folders (from M5's plan)

- **Need:** pick up what a copier drops into a folder, separate it and
  file it.
- **Where:** `m5-intake.md:1546-1549` ("Watched folders … M15's batch
  actions, on `sheaf separate`").
- **Fit:** the same as entry 88 (notify). Copier scan-to-folder targets
  are often network shares, where notify's docs warn that events may not
  arrive.
- **Recommendation:** as entry 88.

### M16: Agent tools (MCP), and the agent panel (Pending)

#### 75. The MCP server (`sheaf mcp`)

- **Need:** a stdio MCP server, relayed to the running app, serving both
  protocol eras.
- **Where:** `ROADMAP.md:491-494`; `todos/agent.md:99-121`, `:123-305`.
- **Today's plan:** "a stdio MCP server (`rmcp`)" (`ROADMAP.md:491`); rmcp
  "shipped breaking majors … so pin it exactly" (`agent.md:175-176`).
- **Candidates:** [rmcp](https://crates.io/crates/rmcp) 3.7.0
  (2026-10-22, two days after the 3.6.0 the memo checked; Apache-2.0;
  39M downloads; the official SDK,
  [modelcontextprotocol/rust-sdk](https://github.com/modelcontextprotocol/rust-sdk)).
  3.7.0's notes list auth additions and two fixes, plus a "Merge commit
  from fork" (possibly a security fix; unverified). Its
  `ProtocolVersion` has `V_2026_07_28` as `LATEST` and `V_2025_11_25`
  (`crates/rmcp/src/model.rs:167-174`); MCP's current revision is still
  [2026-07-28](https://modelcontextprotocol.io/specification/versioning).
  Needs tokio; its schemars support is schemars 1.0.
- **Fit:** tokio is in sheaf's default build already: wasmtime-wasi 49
  and wasmtime-wasi-http 49 depend on it (tokio 1.53.2 in the lock),
  through sheaf-plugin, which the default `plugins` feature brings. So
  option 7's "Keeps rmcp and tokio out of the GUI" (`agent.md:234`)
  saves less than it suggests. schemars 1.x matches `sheaf-edit`'s.
- **Recommendation:** **adopt rmcp**, pinned exactly, as planned; recheck
  its version and the `allowed_origins` default at kickoff. High.

#### 76. The ACP client (the agent panel)

- **Need:** launch Claude Code or Codex adapters over ACP: sessions,
  streamed updates, permission requests.
- **Where:** Pending "Agent panel" (`ROADMAP.md:784-815`);
  `agent.md:93-97`.
- **Today's plan:** "crate `agent-client-protocol`, Apache-2.0, 3.3.0 on
  2026-10-14" (`agent.md:94-95`).
- **Candidates:** [agent-client-protocol](https://crates.io/crates/agent-client-protocol)
  3.5.0 (2026-10-22; Apache-2.0; 5.5M downloads; six minor releases since
  3.0.0 on 2026-10-06, majors 1 to 3 since July);
  [agent-client-protocol-rmcp](https://crates.io/crates/agent-client-protocol-rmcp)
  4.0.5 (2026-10-22). The SDK workspace also has `-conductor`, `-http`,
  `-polyfill` and `-trace-viewer` crates (unreviewed).
- **Fit:** the protocol's own SDK; the churn is the cost.
- **Recommendation:** **adopt**, pinned exactly; take the version at
  kickoff. High.

#### 77. JSON Schema for ops and commands

- **Need:** argument schemas shared by MCP tools, M15 steps, plugins and
  key bindings.
- **Where:** `ROADMAP.md:516-518`; F5 (`backlog.md:512-572`);
  `agent.md:375`.
- **Today's plan:** "schema comes from schemars, as `plugin::Run`'s
  already does" (`agent.md:375`).
- **Candidates:** [schemars](https://crates.io/crates/schemars) 1.2.3
  (2026-10-12; MIT; 574M downloads); sheaf locks 1.2.2, in `sheaf-edit`,
  `sheaf-app` and `sheaf-cli`.
- **Recommendation:** **adopt** (already adopted). High.

#### 78. The local transport (relay socket, Windows pipe)

- **Need:** `sheaf mcp` relays to `agent.sock`. On Windows the plan
  chooses between bridge threads and a per-user named pipe with a peer
  check.
- **Where:** `agent.md:189-305` (decisions 1–3): "Windows: bridge
  threads, or a per-user named pipe?" (`:292-293`). D16.
- **Candidates:** tokio's `net::windows::named_pipe`, in the build
  already; [interprocess](https://crates.io/crates/interprocess) 2.4.6
  (2026-10-16; 0BSD OR Apache-2.0; 16.5M downloads; local sockets on
  every OS, with tokio support); Greg's
  [instance-handoff](https://crates.io/crates/instance-handoff)
  0.1.0-alpha.1, whose socket code is the precedent (entry 96).
- **Fit:** tokio's named pipes cover the pipe option with no new
  dependency; the SID checks are sheaf's own either way (entry 80).
  interprocess would unify the Unix and Windows paths, but adds a
  dependency for what instance-handoff already solved once.
- **Recommendation:** **build on tokio** (named pipes on Windows, Unix
  sockets elsewhere), sharing the folder and peer checks with
  instance-handoff. Medium. Entries 79–80 have the checks' detail, from
  the security section.

#### 79. Same-user checks on the agent socket (Unix)

- **Need:** refuse any peer on `agent.sock` that isn't the same user, on
  Linux and macOS.
- **Where:** M16: "a 0700 runtime folder the user must own, and a peer
  uid check on every connection" (`agent.md:198-202`).
- **Today:** sheaf-instance calls libc's `SO_PEERCRED` on Linux and
  `getpeereid` on macOS, with `unsafe`
  (`crates/sheaf-instance/src/lib.rs:398-439`). PR #32 replaces
  sheaf-instance with instance-handoff.
- **Candidates:** [tokio](https://crates.io/crates/tokio) 1.54.1 (MIT):
  `UnixStream::peer_cred()` returns a `UCred` with uid, gid and pid
  (`tokio-1.54.0/src/net/unix/stream.rs:981`); safe, and rmcp runs on
  tokio. std's `UnixStream::peer_cred` is **still nightly-only**
  (`peer_credentials_unix_socket`,
  [#42839](https://github.com/rust-lang/rust/issues/42839);
  [docs](https://doc.rust-lang.org/std/os/unix/net/struct.UnixStream.html)).
  interprocess 2.4.6's `PeerCreds` (euid on Unix).
- **Recommendation:** **adopt tokio's `peer_cred`** for the agent socket.
  High.

#### 80. Same-user checks on Windows (named pipes or AF_UNIX)

- **Need:** only the same user may connect; the relay checks the
  server's SID before sending anything, against another user squatting
  the name; remote clients are rejected, and the pipe is created as the
  first instance.
- **Where:** M16: `agent.md:274-282`, and decision 2 at `:291-292`
  ("bridge threads, or a per-user named pipe? And which peer check lands
  before client data flows?"). D16.
- **Candidates:**
  - **tokio's named pipes:** `ServerOptions::first_pipe_instance`,
    `reject_remote_clients` and `create_with_security_attributes_raw`
    (`unsafe`; `tokio-1.54.0/src/net/windows/named_pipe.rs:2057-2310`).
  - **interprocess 2.4.6** (606★): named pipes and local sockets with
    security descriptors, `FILE_FLAG_FIRST_PIPE_INSTANCE` and
    `PIPE_REJECT_REMOTE_CLIENTS`; `client_process_id`,
    `server_process_id`, `client_session_id` and `impersonate_client`
    (`src/os/windows/named_pipe/stream/impl.rs:57-132`). Its
    cross-platform `PeerCreds` gives only a pid on Windows.
  - **uds_windows** 1.2.1 (already used) plus the `SIO_AF_UNIX_GETPEERPID`
    ioctl, defined in mingw-w64's
    [`afunix.h`](https://mingw.googlesource.com/mingw-w64/+/609a2498c115ccc355ade1638499d10ed36a940e/mingw-w64-headers/include/afunix.h)
    and used by [GLib](https://src.opensuse.org/luc14n0/glib/src/tag/2.72.0/gio/giowin32-afunix.h);
    Microsoft's documentation for it wasn't found. It would keep today's
    socket.
  - **The SID comparison has no crate:** OpenProcess → OpenProcessToken →
    GetTokenInformation(TokenUser) → EqualSid, or
    ImpersonateNamedPipeClient → OpenThreadToken. About 50 lines on the
    windows crate, which sheaf already depends on.
- **Recommendation:** **build the SID check on the windows crate,** over
  tokio's named pipes; interprocess if a synchronous API is wanted.
  Medium.

### M17: Accessibility (and M7's text with layout)

#### 81. The app with a screen reader (AccessKit through gpui-pre)

- **Need:** roles, labels and states on every control; announcements; a
  tree of its own for the drawn page view; a way to test it all.
- **Where:** `ROADMAP.md:546-574`.
- **Today's plan:** "GPUI (gpui-pre 0.3.9) reports its UI through
  AccessKit … An element with an id and a role (`.role(Role::Button)`) is
  reported" (`:549-552`). "Open: how to test it. GPUI has an `a11y` debug
  module, and AccessKit trees can be read back; neither is explored yet"
  (`:572-574`).
- **What gpui-pre 0.3.9 has** (its registry source,
  `src/_accessibility.rs` and `src/window.rs`):
  - `.role()` and `on_a11y_action(AccessibleAction::…)`; `.on_click()`
    registers `Click` by itself.
  - `Element::a11y_synthetic_children` with an `A11ySubtreeBuilder`: "a
    custom Element may want to appear as if it is really made of multiple
    nodes". The hook for the page view's text, marks and fields; it runs
    after prepaint, so it knows what's visible.
  - `Window::debug_a11y_tree_json()` (`window.rs:7011`): "the last
    frame's accessibility information".
  - `is_a11y_active()` is true only while assistive technology is
    connected; `Application::new_inaccessible` turns a11y off.
  - Whether a headless `#[gpui::test]` can build a tree without an AT
    connected is unverified: no a11y code was found in its test platform.
- **Candidates:** [accesskit](https://crates.io/crates/accesskit) 0.25.1
  (2026-09-25; MIT OR Apache-2.0, its adapters too). sheaf locks 0.24.1
  through gpui-pre, with accesskit_windows 0.34.0, accesskit_macos 0.26.3
  and accesskit_unix 0.22.1 (AT-SPI). `accesskit_consumer` 0.38 (in the
  lock) can walk a tree in tests.
- **Fit:** everything goes through GPUI, so there's no direct dependency
  to choose. Flatpak needs access to the a11y bus (unverified; check with
  the packaging work).
- **Recommendation:** **adopt** (through gpui-pre). Spike first:
  `debug_a11y_tree_json` in a headless test, and
  `a11y_synthetic_children` on the page view. Medium-high.

#### 82. PDF/UA validation

- **Need:** check PDF/UA's machine-checkable rules on a file, and support
  a review view for the human ones.
- **Where:** `ROADMAP.md:576-602`; F9.
- **Today's plan:** "The Matterhorn Protocol lists PDF/UA's failure
  conditions; veraPDF validates both standards, and is the reference to
  compare against" (`:581-584`).
- **Candidates:** veraPDF 1.30 (`ua1`, `ua2`, `wt1r`, `wt1a`; entry 68).
  The Matterhorn Protocol: 31 checkpoints and 136 failure conditions, 89
  that software can assess and 47 that need a person
  ([PDF Association](https://pdfa.org/?p=2162)); its edition is for
  PDF/UA-1, and a UA-2 edition is unverified. PAC (axes4): Windows
  freeware, closed, not usable in CI (a CLI is unverified). pdf_oxide's
  `src/compliance/pdf_ua.rs` (28 KB, not reviewed). oxidize-pdf makes no
  PDF/UA claim.
- **Fit:** as with PDF/A, no Rust validator is close to veraPDF. The
  human checks need sheaf's own review view.
- **Recommendation:** **build in-house** for the checks covering what
  sheaf writes and for the review view; **veraPDF in tests** (`ua1`,
  `ua2`) over `conformance/`. Medium. Whether it shares M14's validator
  ("(open)" in the dependency table) depends on whether M14 builds one; a
  common rule-table format, krilla style, serves both.

#### 83. Text structure: reading order, ActualText, alt text, text with layout

- **Need:** reading order on untagged pages; ActualText and Alt from
  marked content and the structure tree; the page view's accessible
  text; M7's export "with layout ❓"; the Markdown export (Pending).
- **Where:** `ROADMAP.md:559-561` (the page view's tree), `:577-583`
  (structure), `:592-594` (tagging); `m11.md:174-176`; `ROADMAP.md:185`.
- **Today's plan:** the content interpreter ties content to tags
  (`ROADMAP.md:579-580`); oar-ocr's PP-DocLayoutV2 orders regions
  (`:592-594`); PDFium supplies the characters.
- **Candidates:** no Rust crate reads and edits a PDF structure tree on
  an existing file; krilla writes tags only into new files.
  pdfplumber-core's word grouping was already judged weak
  (`docs/prior-art/rust-ecosystem.md:255-260`).
- **Recommendation:** **build in-house** on the M11 interpreter
  (`marked()` already lexes BDC properties) plus oar-ocr, already
  adopted. Medium. No crate fits.

### Pending

#### 84. Spell check

The fonts section's entry; core checked the same facts.
- **Need:** underline misspellings, with suggestions and "Add to
  dictionary", in `TextInput`/`TextArea`, notes and fields, with a tax
  word list.
- **Where:** `ROADMAP.md:839-865`. D15.
- **Today's plan:** engines listed (`:847-854`): the platform checkers;
  "spellbook (Helix's pure-Rust, Hunspell-compatible checker; MPL-2.0,
  alpha, no phonetic suggestions)"; "zspell (pure Rust); its license and
  suggestion API are unchecked."
- **Candidates:**
  - **[spellbook](https://crates.io/crates/spellbook) 0.4.2**
    (2026-06-03; MPL-2.0; helix-editor (the-mikedavis); 254k downloads;
    3 releases in 12 months; pushed 2026-07-11). The README says it
    "should be considered to be in _alpha_"; `check` "works well for
    `en_US`"; `suggest` "should behave the same as Nuspell's"; no
    phonetic suggestions; complex compounding "may work less well".
    `no_std`; only hashbrown (plus foldhash by default); about 10
    `unsafe` blocks (`get_unchecked` in the suggester and parser). Users:
    codebook (a Zed spell-check extension) and cargo-spellcheck.
    **MPL-2.0 is file-level copyleft:** static linking is allowed, but
    sheaf must offer the source of spellbook's files, with any changes. A
    cost, not a blocker.
  - **[zspell](https://crates.io/crates/zspell) 0.5.5** (2024-06-13):
    **Apache-2.0** (its LICENSE file; crates.io shows "non-standard").
    The checker is stable; suggestions are behind `unstable-suggestions`
    ("not yet finalized"). Last commit 2024-07-09; 150k downloads.
    Stale.
  - **[hunspell-rs](https://crates.io/crates/hunspell-rs) 0.4.0 (2022)
    and hunspell-sys 0.3.1 (2022):** FFI to C++ Hunspell (MPL-1.1/GPL-2/
    LGPL-2.1 tri-license). Stale.
  - **Platform:** Windows'
    [`ISpellChecker`](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Globalization/struct.ISpellChecker.html)
    (`windows` crate; Windows 8+) and macOS's
    [`NSSpellChecker`](https://docs.rs/objc2-app-kit/latest/objc2_app_kit/struct.NSSpellChecker.html)
    (`objc2-app-kit`).
  - **Dictionaries:** LibreOffice's en_US is SCOWL size 60 (2020.12.07),
    permissive: "Permission to use, copy, modify, distribute and sell
    these word lists … provided that the above copyright notice appears
    in all copies"; its affix file is under Geoff Kuenning's BSD license
    ([README_en_US](https://raw.githubusercontent.com/LibreOffice/dictionaries/master/en/README_en_US.txt)).
    Other languages vary (LGPL, GPL, MPL): load them from
    `/usr/share/hunspell` rather than bundling them.
- **Fit:** the platform checkers give users their own languages and word
  lists. spellbook is the only maintained pure-Rust engine with
  suggestions, for Linux, or as one engine everywhere. Nothing here
  touches PDF bytes.
- **Recommendation:** **adopt behind a trait:** platform checkers on
  Windows and macOS; spellbook on Linux, or everywhere if one behavior is
  wanted; bundle SCOWL en_US plus sheaf's tax word list; drop zspell.
  Medium. What decides it: whether firms want identical suggestions on
  every OS.

#### 85. Search: case folding and Unicode equivalence

- **Need:** Find, Patterns and the Pending `sheaf search`
  (`ROADMAP.md:817-834`) should match what a user means:
  case-insensitively; across ligatures (`ﬁ` U+FB01 from a ToUnicode or
  OCR); across compatibility forms (full-width digits in an EIN); across
  NFC/NFD (macOS-typed queries against producer text).
- **Where:** `crates/sheaf-doc/src/page_chars.rs:68-80` (`Query::literal`,
  `case_insensitive(!match_case)`); M10.5 slice 19's notes
  (`m10.5-edit-api.md:17615-17631`). B29.
- **Today's plan:** "the regex crate's case folding instead of a
  per-character lowercase" (`m10.5-edit-api.md:17630-17631`).
- **Candidates:** regex: "Case insensitive searching is Unicode-aware and
  uses simple case folding" ([docs](https://docs.rs/regex/latest/regex/#unicode)),
  so ß doesn't match SS (inferred from simple folding; unverified);
  unicode-normalization (NFKC; already a dependency);
  [caseless](https://crates.io/crates/caseless) 0.2.2 (MIT, 2024-12; full
  case folding); [icu_casemap](https://crates.io/crates/icu_casemap)
  2.4.0 (Unicode-3.0).
- **Fit:** case is handled. Whether PDFium's text page already expands
  U+FB01 is unverified. Equivalence needs NFKC (or a narrower ligature
  map) on both the page text and the query, with an offset map back to
  characters; `PageChars` already maps byte offsets to characters.
- **Recommendation:** **build on unicode-normalization,** after a
  fixture confirms what PDFium returns. Low.

#### 86. Export to Markdown, and the agent panel's Markdown

- **Need:** write headings, paragraphs, lists and tables in reading order
  (export); render streamed Markdown replies (the panel).
- **Where:** Pending "Export to Markdown" (`ROADMAP.md:686-705`);
  `agent.md:437`.
- **Today's plan:** export: "born-digital pages use PDFium's text plus
  heuristics … Scans can use the structure pipeline in `oar-ocr` … with
  `to_markdown()`" (`ROADMAP.md:698-703`). Panel: "our own Markdown
  renderer on `pulldown-cmark` (MIT)" (`agent.md:437`).
- **Candidates:** [pulldown-cmark](https://crates.io/crates/pulldown-cmark)
  0.13.5 (2026-10-21; MIT; 184M downloads; pure Rust);
  [comrak](https://crates.io/crates/comrak) 0.57.0 (2026-10-21;
  BSD-2-Clause), with a formatter from a CommonMark/GFM tree back to
  text.
- **Fit:** export is a serializer of sheaf's reading-order model; writing
  Markdown (escaping, GFM tables) is small. comrak's tree would be a
  second, intermediate model. pulldown-cmark can parse the output back in
  tests.
- **Recommendation:** export: **build in-house,** with pulldown-cmark in
  tests (medium). Panel: **adopt pulldown-cmark,** as planned (high).

#### 87. Search across files

- **Need:** `sheaf search` over files and folders, and later a GUI.
- **Where:** `ROADMAP.md:817-837`.
- **Today's plan:** "Each file opens as a `Session` and runs
  `Text::search` … Pages with no text are counted, not OCR'd. Encrypted
  files are listed and skipped unless a password is given" (`:825-829`).
- **Candidates:** for walking,
  [walkdir](https://crates.io/crates/walkdir) 2.5.0 (2024-03-01;
  Unlicense OR MIT; 711M downloads) or
  [ignore](https://crates.io/crates/ignore) 0.4.33 (2026-08-04). For an
  index, if ever: [tantivy](https://crates.io/crates/tantivy) 0.27.0
  (2026-10-17; MIT; 16k stars; pure Rust, heavy), or SQLite FTS5 through
  [rusqlite](https://crates.io/crates/rusqlite) 0.40.2 (MIT; C SQLite
  bundled).
- **Fit:** the CLI needs no index; regex and Patterns are in place. An
  index of client PDFs would be a plaintext copy of their text on disk,
  which runs into M11's "No plaintext copies of an encrypted document on
  disk" (`m11.md:229`), and it's client data at rest besides.
- **Recommendation:** **build in-house** on `Session`, with walkdir (or
  `std::fs`). Defer any index; if the GUI needs repeat searches over one
  client's folder, decide between tantivy and FTS5 then, with the index's
  encryption and location as the deciding constraints. High for the CLI.

#### 88. File watching and file contention

- **Need:** notice that a file changed on disk while open (the banner
  when the window comes to the front; the lock file's "Notify Me"); M15's
  watched folders (entry 74).
- **Where:** Pending "File contention" (`ROADMAP.md:768-782`);
  `todos/file-contention.md:111-112`, `:129-131`, `:133-150`.
- **Today's plan:** layer 1 ("detect changes before saving") is done in
  M10.5, with a `FileStamp`. "When a window comes to the front, check the
  file" (`file-contention.md:111`). On a network share, the lock holder
  is found through OS locks, or by a heartbeat where locks don't reach
  the server (`:133-150`). `settings.toml` is reloaded by an mtime stamp
  (`crates/sheaf-app/src/prefs.rs:73-75`).
- **Candidates:** [notify](https://crates.io/crates/notify) 8.2.0
  (2025-08-03, the latest stable; 9.0.0-rc.6 on 2026-10-17). The `notify`
  crate is CC0-1.0; notify-types and the debouncers are MIT OR
  Apache-2.0. 179M downloads; inotify, FSEvents and
  ReadDirectoryChangesW, with a polling backend. Its docs: "Network
  mounted filesystems like NFS may not emit any events … A workaround is
  the `PollWatcher` backend" (`notify/src/lib.rs:37-42`). For lock files,
  [fs4](https://crates.io/crates/fs4) 1.1.0 (MIT OR Apache-2.0) or
  [fd-lock](https://crates.io/crates/fd-lock) 4.0.4; std's `File::lock`,
  stable in recent Rust (unverified for sheaf's 1.98), may make both
  unnecessary.
- **Fit:** the case the design cares about is a colleague on a file
  server, and events from another SMB or NFS client are exactly what
  notify can't promise, so polling the `FileStamp` (on focus, and every
  few seconds while focused) is the reliable base. notify adds instant
  updates for local and sync-client changes. CC0 is a public-domain
  dedication, compatible with sheaf's licenses; some legal reviews flag
  that it waives no patents, a small cost.
- **Recommendation:** **build** the check on `FileStamp` polling;
  **adopt notify** behind a small trait, with `PollWatcher` as the
  fallback, when Notify Me or watched folders get a slice. Check whether
  std's file locking covers the lock file before adding fs4. Medium.

#### 89. XLSX in or out

- **Need:** none in the plans; only CSV that's "Excel-safe"
  (`m10.md:1014-1017`). Excel export appears only in prior-art notes
  (`docs/prior-art/acrobat.md:331`: "PDF-to-Excel table fidelity is the
  key competitive surface").
- **Candidates, for when it comes:**
  [rust_xlsxwriter](https://crates.io/crates/rust_xlsxwriter) 0.101.0
  (2026-10-20; MIT OR Apache-2.0; the author of Python's XlsxWriter; 129
  releases); [calamine](https://crates.io/crates/calamine) 0.37.0
  (2026-10-18; MIT; reads xlsx, xls and ods).
- **Recommendation:** **undecided.** A plan that exports tables
  (pdfplumber-core) or fills forms from a workbook in M15 decides it;
  then rust_xlsxwriter for output, calamine for input. High that nothing
  is needed now.

#### 90. ML runtimes (UVDoc in use, DocRes pending)

- **Need:** run ONNX models: PP-OCR detection, recognition and
  orientation, and UVDoc (in use, through oar-ocr); DocRes (Pending,
  `ROADMAP.md:707-722`); layout detection for M17 tagging and the
  Markdown export (`:592-594`, `:686-705`).
- **Today:** `ort` 2.0.0-rc.13 with `load-dynamic`, loading Microsoft's
  ONNX Runtime 1.28.0 (`scripts/fetch-ocr-data.sh:18`), pinned by
  oar-ocr's `=2.0.0-rc.13` (`crates/sheaf-ocr/Cargo.toml:31-33`).
- **Candidates:**
  - [`ort`](https://crates.io/crates/ort) 2.0.0-rc.15 (2026-10-19), still
    a release candidate after 16 releases since 2024-02; MIT OR
    Apache-2.0; pykeio; 2.6k stars; FFI to ONNX Runtime (MIT).
  - [`tract-onnx`](https://crates.io/crates/tract-onnx) 0.23.9
    (2026-10-15; MIT OR Apache-2.0; Sonos; 3.1k stars): pure Rust, CPU,
    builds for wasm.
  - [`rten`](https://crates.io/crates/rten) 0.27.0 (2026-10-02; MIT OR
    Apache-2.0; Robert Knight): pure Rust, runs ocrs's models, builds for
    wasm.
  - [`candle-core`](https://crates.io/crates/candle-core) and
    [`candle-onnx`](https://crates.io/crates/candle-onnx) 0.11.0
    (2026-06-26; Hugging Face): partial operator coverage (unverified for
    these models).
  - [`burn`](https://crates.io/crates/burn) 0.22.1 and
    [`burn-onnx`](https://crates.io/crates/burn-onnx) 0.22.0 (Tracel AI):
    ONNX into Rust code at build time.
- **Fit:** a pure-Rust runtime would drop the native ONNX Runtime sheaf
  ships on each platform, and could run models inside plugins. Whether
  tract or rten has the operators and speed for PP-OCRv5 and v6, UVDoc
  and DocRes is unverified. oar-ocr is hard-wired to ort.
- **Recommendation:** **keep ort.** A switch is undecided: it would take
  a spike running PP-OCRv6 detection and recognition and UVDoc on tract
  and rten, measuring operator support, output equality and speed. High
  (keep); low (any switch).

#### 91. Rendering: PDFium, and hayro as a second renderer

The images section's hayro detail, with core's PDFium facts and the
fonts section's notes on hayro's font stack.
- **Need:** a second `Renderer` behind the trait, or a test oracle.
- **Where:** `docs/prior-art/rust-ecosystem.md:18` ("Candidate second
  `Renderer` backend once it matures") and `:234`;
  `m10.5-edit-api.md:6153` ("hayro as an engine: not planned; an oracle
  only"); F17 (hayro-interpret as a second corpus oracle, added by the
  content interpreter PR).
- **PDFium:** through pdfium-render 0.9.4, the latest (2026-09-06).
  sheaf pins the binary at chromium/7881 (`docs/document-model.md:2574`);
  bblanchon's latest build is chromium/8111 (2026-10-19). Nothing to
  change beyond a routine bump.
- **hayro now:** [`hayro`](https://crates.io/crates/hayro) 0.8.0
  (2026-10-04; 9 releases since 2025-07; MIT OR Apache-2.0, GitHub reads
  Apache-2.0; LaurenzV; 814 stars).
  - Very active: about fifteen commits on 2026-10-22 and 23 alone, with
    `hayro-fuzz` and `hayro-bench` in the repository.
  - [README](https://github.com/LaurenzV/hayro): still "experimental,
    work-in-progress"; no knockout groups and no non-embedded CID fonts;
    "performance … has not been a focus at all so far but will become a
    priority in the near future"; 1,400+ test PDFs; MSRV 1.92.
  - Users: Typst's `typst-svg` and `typst-render` (^0.7.1) and
    `typst-library` (hayro-syntax); a dev-dependency of oar-ocr 0.10.1.
  - `hayro` 0.8 renders with `vello_cpu` and `pic-scale`;
    `hayro-interpret` uses `moxcms`, `skrifa` (since 2026-04, dropping
    hayro-font) and `hayro-cmap`.
  - The ecosystem notes' "Pure, `#![forbid(unsafe)]`" (`:234`) is partly
    stale: the codecs' default `simd` feature uses `fearless_simd`, and
    `hayro` itself depends on `fearless_simd` and `pic-scale` (SIMD).
- **Recommendation:** keep PDFium. hayro: **oracle only,** as Greg leans.
  Adopt its codecs now (entry 33) and its renderer not yet; re-check at
  M14's kickoff. Medium.

#### 92. Watch: JPEG XL in PDF

- The PDF Association announced in 2025-11 that it will add JPEG XL as
  the "preferred solution" for HDR images
  ([The Register](https://www.theregister.com/2025/11/10/another_chance_for_jpeg_xl/),
  [heise](https://heise.de/-11073527)). No published ISO 32000 amendment
  was found yet.
- If it lands, PDFium and sheaf's sample decoder both need it:
  [`jxl-oxide`](https://crates.io/crates/jxl-oxide) 0.12.6 (2026-05-29;
  MIT OR Apache-2.0; pure Rust).
- No action now.

### Backlog items

#### 93. B5: a signature already broken still asks

- **Need:** before the signature ask, know whether each signature's
  `/ByteRange` digest still matches what its CMS says (the messageDigest
  signed attribute, or a document timestamp's TSTInfo messageImprint).
  One already broken doesn't ask again.
- **Where:** `backlog.md:184-209` (`forms::sigs::sig_impact`,
  `crates/sheaf-doc/src/forms/sigs.rs:187`); the tests are F9's
  (`:665`).
- **Today's plan:** "probably needs the digest check pyHanko does"
  (`backlog.md:198`).
- **The spike:**
  - Dependencies: cms 0.3.0-rc.0, der 0.8 (features `ber`, `alloc`,
    `oid`), sha1 0.11, sha2 0.11, md-5 0.11 and spki 0.8.
  - Core: about 40 lines. Decode ContentInfo then SignedData with BER
    rules, without calling `finish()`, so the padding is ignored. Take
    the first SignerInfo's `digest_alg` and messageDigest, or the TSTInfo
    imprint, then hash the two ranges.
  - Results: 493 messageDigest matches, 53 timestamp imprint matches, 2
    mismatches. 28 have no signed attributes: their digest is inside the
    signature, so checking them needs entry 45's RSA public operation and
    the certificate. 12 use digests the spike didn't hash (GOST,
    SHAKE256, and one sha1WithRSA OID given as a digest algorithm, which
    pyHanko maps to SHA-1). 5 had parse problems; 18 had `/ByteRange` or
    `/Contents` shapes the spike skips.
  - B5's named files: pyHanko finds neither intact; the spike agrees.
  - Differences from pyHanko's manifest counts, file by file: most come
    from the manifest's pyHanko run failing on document timestamps
    ("Signature object type must be /Sig") or on asn1crypto parse
    errors; others are pyHanko test vectors with unusual ByteRanges,
    which the spike skips.
  - Speed: 173.6 MB hashed in 0.37 s, one thread, release build, parsing
    included.
- **Fit:** small; sheaf already reads the signature dictionaries
  (`forms/sigs.rs`).
- **Recommendation:** **build on cms 0.3.0-rc.0, der 0.8, sha1 and
  sha2,** pinned exactly. High. Compute it lazily, once per source,
  cancellable: a signature covers nearly the whole file, so N signatures
  cost N hashes of it. Count a signature as broken only on a clear
  mismatch; unknown algorithms, no signed attributes and parse failures
  keep asking. Land it with F9's `signatures/` tier.

#### 94. Checking what sheaf writes against the Arlington PDF Model

- **Need:** check every object sheaf builds (annotations, fields,
  StructElems, output intents) against PDF 2.0's model.
- **Where:** F22; F9; M14; M17.
- **Candidates:** the Arlington model's TSVs (Apache-2.0; entry 68). No
  Rust checker exists; TestGrammar (C++) and veraPDF's Arlington service
  do. krilla runs Arlington in CI.
- **Recommendation:** **build** a small test-only reader of the TSVs
  (F22).

### Infrastructure that's built (confirmed)

#### 95. Printing (`sheaf-print`)

- **Where:** M7, done (`todos/m7.md:23-37`;
  `crates/sheaf-print/src/lib.rs:5-9`). B28.
- **Today:** "CUPS through its command-line tools … This avoids linking
  libcups … No desktop portal: it's sandbox-only and offers no preview
  control" (`m7.md:23-30`). Windows uses GDI, with PDFium rasterizing.
- **Candidates:** [printers](https://crates.io/crates/printers) 2.3.0
  (2026-02-06; MIT; libcups on Unix, winspool on Windows; 120k
  downloads). sheaf rejected linking libcups on purpose.
- **Gap:** the Flatpak build is sandboxed. Inside it, `lp` must exist in
  the runtime or be bundled, and the app needs `--socket=cups` (Flatpak
  1.6 and later). Whether the freedesktop runtime ships `lp` is
  unverified ([flatpak-run](https://www.mankier.com/1/flatpak-run),
  [KDE's `--socket=cups`](https://phabricator.kde.org/R256:2415ebe0b7ed42a2eed468c9da10d3924dd9b1c1)).
- **Recommendation:** **build in-house (keep).** Add a Flatpak printing
  check to the packaging work: bundle the CUPS client tools with
  `--socket=cups`, or use the print portal only in the Flatpak. Medium.

#### 96. Single instance and progress (Greg's crates)

- **in-progress** 0.1.0 (2026-10-17; MIT OR Apache-2.0) is used:
  `in-progress.workspace = true` in sheaf-doc, sheaf-ops, sheaf-cli and
  sheaf-plugin (`Cargo.toml:73`).
- **instance-handoff** 0.1.0-alpha.1 (2026-10-21; MIT OR Apache-2.0) is
  published but not used on origin/main, which still has the in-tree
  `crates/sheaf-instance` (libc, uds_windows, windows). PR #32 makes the
  switch: it deletes `crates/sheaf-instance`, and updates `todos/agent.md`,
  which cites `sheaf_instance::default_dir()` at `:164` and
  `crates/sheaf-instance/src/lib.rs` at `:328`.

#### 97. Settings (toml)

- **Today:** `toml = "0.9"` in sheaf-ops and sheaf-plugin (0.9.12
  locked). sheaf never writes settings, so read-only serde is right;
  toml_edit isn't needed.
- [toml](https://crates.io/crates/toml) 1.0.0 came out on 2026-02-11 and
  is at 1.1.8 (2026-10-09; MIT OR Apache-2.0). The lock also has 1.1.8
  (through embed-resource) and 0.8.23 (through cbindgen).
- **Recommendation:** **adopt** (keep), and move to 1.x in a dependency
  refresh, before M15's "settings/keymap format declared stable"
  (`ROADMAP.md:477-478`). High.

#### 98. The CLI (clap)

- **Today:** clap 4 with derive, plus indicatif 0.18 and ctrlc
  (`crates/sheaf-cli/Cargo.toml`); locked clap 4.6.7, the latest
  (2026-09-14).
- **Optional:** [clap_complete](https://crates.io/crates/clap_complete)
  4.6.11 and [clap_mangen](https://crates.io/crates/clap_mangen) 0.3.3
  (both MIT OR Apache-2.0), for shell completions and man pages, if the
  installer or packaging work wants them.
- **Recommendation:** **adopt** (keep). High.

#### 99. Fuzzing and property tests

- **Today:** `fuzz/` has seven libFuzzer targets (`lazy_reader`, `lexer`,
  `op_json`, `page_tree`, `sheaf_annot`, `sheaf_generators`, `splice`) on
  `libfuzzer-sys = "0.4"`; `proptest = "1"` in sheaf-doc, at 1.12.0
  (2026-10-10); cargo-fuzz 0.13.2 (2026-06-09) is the latest.
- **License:** libfuzzer-sys is "(MIT OR Apache-2.0) AND NCSA"; NCSA is
  LLVM's old permissive license, and it's test-only.
- **Recommendation:** **adopt** (keep). For F13, every in-housed reader
  piece gets its fuzz body before it lands, as `source::fuzz` does now.
  High.

#### 100. wasmtime security patches (plugins and the forms guest)

- sheaf locks wasmtime, wasmtime-wasi and wasmtime-wasi-http 49.0.2.
  49.0.3 (2026-10-14) and 49.0.4 (2026-10-19) fix GHSA-mghf-wjfp-p5xm
  (critical; see [Already acted on](#already-acted-on-2026-10-23)), and
  several wasi-http issues. The core section also cited GHSA-5qpp
  (unverified, as above).
- GHSA-w36r (a host panic on long URLs) is in the `http` crate, which
  sheaf locks at 1.5.0, after the fix; whether that's mitigated is
  unverified.
- wasmtime 50.0.0 is out (2026-10-20). The advisories:
  [bytecodealliance/wasmtime](https://github.com/bytecodealliance/wasmtime/security/advisories).
- **Recommendation:** the patch bump to 49.0.4, now: PR #33. Check the
  guest's compile-cache key, which hashes wasmtime's version.

### Needs with no crate, and notes

#### 101. Needs that need no crate

- **Opening links:** gpui-pre has `App::open_url` (`app.rs:1809`), so
  "Follow links in the page view" (`ROADMAP.md:756-766`) needs no crate.
- **Dragging pages between windows** (`ROADMAP.md:865-877`): GPUI work.
- **A binder index page:** an M13 link op.
- **The calculator items F1–F4:** the tape is f64 with sheaf's own
  rounding (`crates/sheaf-edit/src/tape.rs:273-320`). rust_decimal
  1.43.0 (MIT) exists if exact decimal arithmetic is ever wanted; no
  entry asks for it.

#### 102. Table extraction (pdfplumber-core, prior art)

- Not in a milestone. The CSV side uses csv (entry 22); XLSX would be
  entry 89. The table finder plan is in `docs/prior-art/rust-ecosystem.md`.

#### Not covered here

- Code signing of sheaf's own binaries, and the installer's SHA-256
  checks (`todos/code-signing.md`, `todos/installer.md`): packaging.
- Plugin sandbox limits (F11): the plugins area.

## Changes to make, by file

Every plan-line change the four sections proposed, merged. Line numbers
are on `8cdc589` (the M5 plan: `m5-intake` at `4481573`). **This PR
edits no plan,** because other open PRs touch them: the content
interpreter PR edits `todos/m11.md` and `ROADMAP.md`; #31 edits
`ROADMAP.md`; #32 edits `ROADMAP.md`, `todos/agent.md`, `README.md` and
`Cargo.toml`, so lines in those move a little. Each change is
applied at the kickoff of the milestone it belongs to (for M11, which is
under way, at the start of the slice), or by the PR named. Changes that
hang on a decision say which.

### `todos/m10.md`: at M10's kickoff (no open PR edits it)

- **`:590-595` (stale):** M10.5 slice 6 built all 14 AFM tables
  (`content/font/afm.rs`, `std14.rs`, `assets/afm`), and
  `stamps.rs:207-254` no longer exists. Say "built in M10.5" instead of
  "are new".
- **`:607-611` (stale):** the OCR layer's Type 0 dictionaries are now
  `content/font/glyphless.rs`, not `text_layer.rs:395-500`.
- **`:624-627`:** drop "painted in that embedded font if GPUI can load
  it". A PDF's font program isn't handed to CoreText or DirectWrite:
  draw its skrifa outlines as GPUI paths, or use the Liberation Mono or
  Sans fallback. (D9.)
- **`:632-635`:** replace "(for example `subsetter`, `ttf-parser`,
  `unicode-normalization` and `unicode-segmentation`; licenses to
  check)" with: skrifa/read-fonts for font programs (TrueType, CFF and
  Type 1); a GID-preserving `glyf` subset, in-house on read-fonts or
  skera with `RETAIN_GIDS`, not subsetter, which renumbers glyphs (D1);
  unicode-normalization, already sheaf-doc's; unicode-segmentation,
  already sheaf-ui's; unicode-bidi for the right-to-left check.
- **`:1455-1460` (slice 7):** name the subsetter decision (D1). Add: Noto
  Sans unhinted (431 KB, 3,884 glyphs), OFL with no Reserved Font Name.
- **`:1469` and `:1597`:** `packaging/licenses` gains `LICENSE-noto`
  (OFL), `LICENSE-liberation` (OFL; RFNs "Liberation", "Arimo",
  "Tinos", "Cousine"; shipped unmodified), and the Unicode-3.0 notice
  for unicode-normalization ≥ 0.1.26 and ICU4X data (F18). (Adobe's CMap
  notice lands with the content interpreter PR.)
- **`:1584-1590` (slice 11):** after "paints it by proposal 11's
  editor-font rule", add "as glyph runs from slice 6's layout through
  `Window::paint_glyph`, so the editor and the appearance place every
  glyph alike". (D9.)

### `todos/m11.md`: after the content interpreter PR merges, at each slice's start

- **`:459-463` (slice 3, Fonts):** add font programs through skrifa where
  the descriptor lacks metrics (PDFium's `CheckFontMetrics` takes
  `FontBBox`, Ascent and Descent from the program, or from the boxes of
  'A' and 'g'; `LoadFontDescriptor` negates a Descent above 10);
  predefined CMaps through hayro-cmap for Type0 fonts not on Identity
  (code length, code → CID for `/W`, CID → Unicode through
  Adobe-\*-UCS2); a corpus check on a CJK file. **Who:** the content
  interpreter PR, whose wrap-up takes skrifa and hayro-cmap; check it
  carries the `CheckFontMetrics` rule and the CJK check.
- **`:72-73`** (decision 3's "certificate stores (PKCS#12, the Windows
  store, PKCS#11)"): add the macOS keychain. **Who:** with the ROADMAP
  change at M12's kickoff (D8).
- **`:389-391`** ("#584 is left for slice 1") **and `:424`** ("Work
  around lopdf #584 for reading"): stale. The fix is upstream `0e781ce`
  (PR #573), in the vendored `39492cb` and in lopdf 0.46.0
  (`EncryptionState::decode` calls `resolve_password_for_key_derivation`,
  `vendor/lopdf/src/encryption.rs:623-624`). Keep only the check. #603 is
  also fixed upstream, in 0.46.0 (`2f497f8`). Issue #584 still shows
  open on GitHub and could be closed. **Who:** the next PR that edits
  m11.md.
- **`:425` and its Check at `:429-432`:** test the certificate-encryption
  refusal on the corpus's 12 `/Adobe.PubSec` files
  (`signatures/pyhanko/pdf/*pubkey*`, `signed-encrypted-pubkey-*`).
  **Who:** the next PR that edits m11.md.
- **`:488-493` (slice 5), `:562-564`, and the docs line `:536-537`:**
  blank the outlines of glyphs whose codes are "no longer used anywhere"
  (D11). **Who:** slice 5's start, if D11 is accepted.
- **`:152`:** replace "PDFium decodes, since it handles every filter."
  with: "sheaf decodes the samples itself, in the image's own color space
  and bit depth: lopdf for the stream filters, zune-jpeg for DCT,
  hayro-ccitt, hayro-jbig2 and hayro-jpeg2000 for the rest. PDFium's
  `FPDFImageObj_GetBitmap` converts to BGR or gray, and
  `GetImageDataDecoded` stops at image filters. PDFium renders the result
  as the check." (Both the images and security sections; D5.) **Who:**
  slice 6's start.
- **`:152-154`:** "JPEGs become JPEG again" says how: with `jpeg-encoder`
  (CMYK, the original's quantization tables and subsampling), with no
  APPn segments carried over. Add "JPX becomes Flate; JBIG2 becomes G4;
  JBIG2 is never written", and that the report says which images were
  re-encoded. **Who:** slice 6's start (D5).
- **`:155-157`:** add "An image's `/JBIG2Globals` goes when nothing else
  uses it; when another image shares it, [D5's answer]. 15 of the
  corpus's 127 JBIG2 scans share globals." and "The fill is converted
  into the image's color space, honoring `/Decode`." **Who:** slice 6's
  start.
- **`:160-163`:** replace "with a polygon-boolean crate such as `geo` or
  `i_overlay` (licenses to check)" with: i_overlay 9.x (MIT OR
  Apache-2.0; i_float and i_shape MIT), used directly, not through geo,
  whose 0.33.1 pins i_overlay 4.5; kurbo, already in the lockfile, for
  flattening and stroke expansion; clipper2-rust (BSL-1.0) as a second
  engine in the corpus check; coordinates checked (finite, bounded) and
  segment counts capped before the float API, which panics otherwise;
  and D6's answer on strokes. (The security section; core's license note
  is the same.) **Who:** slice 6's start.
- **`:497-505` (slice 6):** add "the image sample decoder (a
  `sheaf-image` module, also used by M13, M14 and M5's separator
  fallback)" to the slice, and make its check read the image data back
  through that decoder. **Who:** slice 6's start.
- **`:242`:** metadata. Removing XMP from a PDF/A or PDF/UA file voids
  its claim: write a minimal fresh packet keeping `pdfaid` and `pdfuaid`,
  with Producer and ModifyDate, or say the claim goes (D12). Read it
  with quick-xml, as sheaf-doc does now. **Who:** slice 9's start.
- **`:250-251`:** actions and JavaScript: add `/AA` on pages, annotations
  and fields; follow `/Next` chains; add SubmitForm, ImportData, GoToR,
  GoToE and RichMedia; XFA `<script>` elements in the template packet
  (quick-xml), unless the form-fields category removes XFA. **Who:**
  slice 9's start.

### `ROADMAP.md`: at each milestone's kickoff (the content interpreter PR, #31 and #32 edit it now)

- **M12, `:396` (Fill & Sign):** add "a bundled script font for typed
  signatures (license to check), embedded through M10 slice 7's
  subsetting".
- **M12, `:397-398` and slice 4 at `:404-405`:** add the macOS keychain
  (security-framework) beside the Windows store and PKCS#11 tokens (D8).
- **M12, `:401`:** LTV comes in two forms, and verification reads both:
  the PDF `/DSS` with `/VRI` (PAdES B-LT, B-LTA), and Adobe's
  `adbe-revocationInfoArchival` signed attribute, which 144 of the
  corpus's 593 signatures carry.
- **M12, a "Decide at kickoff" line:** are cms 0.3, x509-cert 0.3,
  x509-ocsp 0.3 and x509-tsp 0.2 published by then (D2)? p12-keystore on
  the published cms? Signing with PKCS#12 keys through ring or the rsa
  crate (RUSTSEC-2023-0071)? Trust anchors: user-trusted, the OS stores,
  the EU list (D13)? The facts behind them: 23% of the corpus's
  real-world signatures are BER, and the stable cms 0.2.3 reads none.
- **M12, `:411-415` (certificate encryption):** name the pieces: cms
  EnvelopedData; RSA key transport (PKCS#1 v1.5, OAEP) and ECDH (X9.63);
  des, rc2 and aes; decryption through the key-store trait; the 12 corpus
  files and pyHanko's test keys.
- **M13, `:418-420`:** add "shaping with harfrust (rustybuzz is
  archived), line breaks from icu_segmenter. Existing subsets rarely have
  the glyphs or the layout tables, so new text that needs them goes in a
  new font resource. Installed fonts are found through fontdb."
- **M14, `:434-435` (Compare):** add `similar` for text, with its
  deadline (imara-diff if a benchmark says); a pixelmatch-style kernel
  (`blazediff`, or a port); not `dssim` (AGPL-3.0); the report on
  sheaf's own writer.
- **M14, `:436-437` (Optimize):** add: re-subsetting keeps codes (unused
  glyphs are blanked in place; no crate subsets the bare CFF or Type 1
  programs PDFs embed); image work runs on M11's sample decoder; JPEG
  with `jpeg-encoder`, bilevel as G4, never lossy JBIG2; downsampling
  with `fast_image_resize` if it measures faster; Flate on zlib-rs, with
  any "maximum" setting (zopfli or libdeflate) decided by measurement.
- **M14, `:455-456` (PDF/A):** add the testing plan (the veraPDF CLI over
  `../pdf-corpus/conformance/`, a test tool and never linked; D3); the
  output intent (D14: the CC0 Compact-ICC-Profiles, or one generated by
  moxcms, already in the tree); PDF/A-1 conversion re-encoding LZW and
  JPX through M11's decoder.
- **M17, `:572-574`** ("Open: how to test it"): name what exists:
  `Window::debug_a11y_tree_json()` (gpui-pre 0.3.9 `window.rs:7011`),
  `accesskit_consumer` for walking a tree, `Element::a11y_synthetic_children`
  as the page view's hook. The open part becomes whether headless tests
  can activate the tree.
- **M17, `:581-584`** (Validate): add veraPDF's `ua1` and `ua2` flavours
  (1.30), and the Matterhorn split: 136 failure conditions, 89 that
  software can check and 47 that need a person. The review view exists
  for the 47.
- **Pending, `:817-837`** (Search across files): add to Open that a GUI
  index would be a plaintext copy of client text on disk (M11 proposal
  11), so an index needs its own decision on encryption and location.
  **Who:** when the entry leaves Pending.
- **Pending, `:850-854`** (Spell check): zspell is Apache-2.0 (its
  LICENSE file), last released 0.5.5 on 2024-06-13, last commit
  2024-07-09, suggestions behind an unstable feature: drop it. spellbook
  0.4.2 (2026-06-03): MPL-2.0 is file-level copyleft (offer its source);
  a few `unsafe` blocks. LibreOffice's en_US (SCOWL) is permissive.
  **Who:** when the entry leaves Pending (D15).

### `todos/agent.md`: at M16's kickoff (#32 edits it now, so lines move)

- **`:94-95`, `:172`, `:175-176`, `:183`:** versions: ACP 3.5.0 and
  agent-client-protocol-rmcp 4.0.5 (both 2026-10-22); rmcp 3.7.0
  (2026-10-22).
- **`:200-202`:** the agent socket's uid check is tokio's
  `UnixStream::peer_cred`; std's is still unstable.
- **`:234`** (option 7, "Keeps rmcp and tokio out of the GUI"): tokio is
  already in sheaf-app's default build, through sheaf-plugin's
  wasmtime-wasi and wasmtime-wasi-http (tokio 1.53.2).
- **`:274-282`:** name the parts: tokio's or interprocess's named pipes
  (first instance, reject remote, security descriptor, client and server
  pid, impersonation), or uds_windows with `SIO_AF_UNIX_GETPEERPID`; the
  SID check is sheaf's own code on the windows crate (D16).
- **`:164`, `:200`, `:328`** (`sheaf-instance`): **done by #32,** whose
  agent.md already names instance-handoff there.

### `todos/m10.5-edit-api.md`: at M14's kickoff

- **`:3667`:** after "Metric-compatible substitutes (Liberation)", add
  "Liberation covers Helvetica, Times and Courier; Symbol and
  ZapfDingbats need Foxit's substitutes (BSD-3, as PDFium and pdf.js
  ship them)" (D10).

### `todos/m5-intake.md` (the `m5-intake` branch): by the M5 work, on its branch

- **`:187-188` and `:621-624`:** add "re-checked 2026-10-23: `libsane-sys`
  doesn't exist, and no maintained SANE crate does either." The decision
  doesn't change.
- **`:282-293`:** a note under F16: "rxing in a plugin needs a
  wasm32-wasip2 build check; rxing-wasm targets wasm32-unknown-unknown."

### `README.md`: at the kickoffs named

- **`:365-366`:** "Scripting language for batch actions: Rhai … vs Lua"
  is listed as open, but `ROADMAP.md:466-468` settled it (plugins first,
  Rhai optional). Mark it settled; `:304`'s "(Lua or Rhai, not JS)"
  follows. **Who:** M15's kickoff.
- **`:358-363`** ("One parser or two? … Spike in M0"): answered (lopdf
  plus PDFium, then F13). Mark it settled, with a link to F13. **Who:**
  F13's triage.

### `vendor/lopdf/SHEAF.md` and `Cargo.toml`: with the weekend's upstream PRs (Greg, 2026-10-24/25)

- **`SHEAF.md:14`:** "No release has it yet" is stale: lopdf 0.46.0
  (2026-10-21) includes #598. Say so, and that f64 reals, `LazyPdf` and
  the caps are why the vendored copy stays.
- **`SHEAF.md:316-324`** ("Going back to crates.io"): note what upstream
  merged after the vendored commit: #603 (0.46.0, `2f497f8`); #613; #624,
  #625 and #630 (main, 2026-10-22/23). Compare each `sheaf:` hunk with
  upstream before the next move.
- **`Cargo.toml:106-113`:** the comment says the vendoring is because
  "lopdf 0.45.0 reads an earlier revision". Add that 0.46.0 fixes that,
  and that the patch now carries `LazyPdf`, f64 reals, caps and
  encryption.
- **`Cargo.toml` (B26's fix):** one workspace `flate2` entry with
  `zlib-rs` (D4).

### `todos/backlog.md`: in this PR

- **B5** (`:184-209`): the spike's result (entry 93).
- **F9** (`:665-686`): compare the `conformance/` tier with veraPDF's
  JSON; it skips without Java or Docker.
- **F13** (`:726-736`, "Upstream meanwhile"): what upstream has merged,
  and what's left to send (entry 2).
- **F13** (`:744-755`, "Maybe published"): the survey's result (entry 1).
- New entries B24–B29 and F18–F23 (next section).

### `docs/prior-art/rust-ecosystem.md`: in this PR

- **`:18`, `:19` and `:234-239`:** hayro (0.8.0; skrifa and hayro-cmap;
  `forbid(unsafe)` per crate, with SIMD dependencies; an oracle, its
  parts taken one by one), lopdf (vendored and patched; F13; eager
  `Document` verified), pdf-rs (0.10.0, f32, no AI-written PRs), krilla
  (0.8.2; PDF/A-1 to 4 and PDF/UA-1; a reference, not a writer for
  sheaf), typst-pdf (Apache-2.0, verified), pdfium-render (0.9.4); new
  rows for hayro-syntax, pdf_oxide, oxidize-pdf, qpdf-rs, pdf-writer
  (with hayro-write) and xmp-writer; a pointer to this memo.
- The fonts, security and image sections each proposed a verdict table
  of their own for that file. They stay here instead, linked from its
  introduction, so the verdicts live in one place.

## Backlog entries this audit added

In `todos/backlog.md`. B14–B18 (the `m5-intake` branch) and F17 (the
content interpreter PR) are held on other branches; B23 landed with #32
and B30 with #35.
- **B24:** G4 encoding truncates images wider than 65,535 pixels (entry
  38).
- **B25:** fax TIFFs (CCITT Group 3, Modified Huffman) are refused
  (entry 16).
- **B26:** the Flate backend depends on which crates are in the build
  (entry 4, D4).
- **B27:** nothing checks dependencies for advisories or licenses (D7).
- **B28:** printing may not work in the Flatpak build (entry 95).
- **B29:** search misses ligatures, compatibility forms and NFD text
  (entry 85).
- **F18:** notices for the data and code sheaf ships (entries 19, 21, 26,
  35, 63, 84).
- **F19:** wipe file keys, not only passwords (entries 29, 43).
- **F20:** upstream cms `CertificateChoices` alternatives (entry 44).
- **F21:** try PP-OCRv6 in the PaddleOCR engine (entry 13).
- **F22:** check what sheaf writes against the Arlington PDF Model
  (entry 94).
- **F23:** hayro-syntax as a second reader in the lazy-reader corpus
  test (entry 1).
- Lines added to B5, F9 and F13.
- The wasmtime bump the core section proposed as a bug isn't an entry:
  PR #33 is the fix.
