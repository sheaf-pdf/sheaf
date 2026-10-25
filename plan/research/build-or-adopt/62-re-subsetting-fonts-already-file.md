# Build or adopt 62: Re-subsetting fonts already in the file (optimize)

Area: M14: Document processing · Part of [build or adopt](README.md)

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
