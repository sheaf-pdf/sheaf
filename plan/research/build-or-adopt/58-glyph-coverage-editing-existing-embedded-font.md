# Build or adopt 58: Glyph coverage, and editing in an existing embedded font

Area: M13: Content editing · Part of [build or adopt](README.md)

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
