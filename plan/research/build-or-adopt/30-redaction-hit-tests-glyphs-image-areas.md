# Build or adopt 30: Redaction hit tests: glyphs, image areas, annotations

Area: M11: Security and redaction · Part of [build or adopt](README.md)

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
