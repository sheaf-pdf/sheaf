# Build or adopt 23: FDF and XFDF (slice 16)

Area: M10: Forms · Part of [build or adopt](README.md)

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
