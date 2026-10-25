# Build or adopt 59: Shaping and bidi reordering

Area: M13: Content editing · Part of [build or adopt](README.md)

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
