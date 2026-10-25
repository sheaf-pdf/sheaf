# Build or adopt 19: Unicode basics: NFC, grapheme clusters, the right-to-left notice

Area: M10: Forms · Part of [build or adopt](README.md)

- **Need:** NFC at commit; `MaxLen` and comb cells by grapheme cluster;
  detecting right-to-left text for the notice.
- **Where:** M10 slice 7's lowering (`m10.md:1461-1464`); proposal 11
  (`:612-618`).
- **Today's plan:** "Values are normalized to NFC at commit … `MaxLen` and
  comb cells count grapheme clusters … Right-to-left text and complex
  shaping are out, with a notice" (`m10.md:612-615`). The dependencies
  are to be named (`:632-635`).
- **Candidates:**
  - [unicode-normalization](https://crates.io/crates/unicode-normalization)
    0.1.26 (2026-10-16): sheaf-doc's dependency already (locked at
    0.1.25). From 0.1.26 its license is "(MIT OR Apache-2.0) AND
    Unicode-3.0", for its data tables: permissive, but a notice to ship
    (F18). 634M downloads; `no_std`.
  - [icu_normalizer](https://crates.io/crates/icu_normalizer) 2.4.0
    (2026-10-22; Unicode-3.0; ICU4X): in the lock already (2.3.0, through
    `url` → `idna`). No reason to switch.
  - [unicode-segmentation](https://crates.io/crates/unicode-segmentation)
    1.13.3 (2026-06-01; MIT OR Apache-2.0; 629M downloads): sheaf-ui and
    sheaf-app use it (workspace dependency, `Cargo.toml:72`); sheaf-doc
    or sheaf-edit would add it.
  - [unicode-bidi](https://crates.io/crates/unicode-bidi) 0.3.18
    (2024-12-16; MIT OR Apache-2.0; servo; 550M downloads; Unicode 16.0
    tables; one `unsafe`): `bidi_class(c)` gives R/AL for the notice.
- **Fit:** complete for these uses, pure Rust and wasm-friendly.
- **Recommendation:** **adopt** unicode-normalization (keep),
  unicode-segmentation (in sheaf-doc too) and unicode-bidi. High.
