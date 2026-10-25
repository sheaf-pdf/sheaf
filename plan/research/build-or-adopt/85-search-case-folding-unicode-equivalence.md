# Build or adopt 85: Search: case folding and Unicode equivalence

Area: Pending · Part of [build or adopt](README.md)

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
