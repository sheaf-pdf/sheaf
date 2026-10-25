# Build or adopt 22: CSV and tab-delimited data (slice 17)

Area: M10: Forms · Part of [build or adopt](README.md)

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
