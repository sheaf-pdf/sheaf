# Build or adopt 28: Outlines of redacted glyphs (M11's leak; M14's re-subsetting)

Area: M11: Security and redaction · Part of [build or adopt](README.md)

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
