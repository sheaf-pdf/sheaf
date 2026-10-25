# Build or adopt 25: Font programs: outlines, metrics and boxes

Area: M11: Security and redaction · Part of [build or adopt](README.md)

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
  ([Already acted on](audit-notes.md#already-acted-on-2026-10-23)). m10.md decision
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
