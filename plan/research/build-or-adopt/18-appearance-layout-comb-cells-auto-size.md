# Build or adopt 18: Appearance layout: comb cells, auto size, wrapping

Area: M10: Forms · Part of [build or adopt](README.md)

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
