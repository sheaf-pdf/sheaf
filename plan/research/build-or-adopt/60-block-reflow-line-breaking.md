# Build or adopt 60: Block reflow and line breaking

Area: M13: Content editing · Part of [build or adopt](README.md)

- **Need:** re-break a text block after an edit: UAX #14 break
  opportunities, fit to the block's width in the PDF fonts, PDF text
  state kept (`Tc`, `Tw`, `Tz`, `TL`).
- **Where:** M13 "block reflow" (`ROADMAP.md:428`).
- **Today's plan:** none; `wrap_with` breaks only at spaces.
- **Candidates:**
  - **[icu_segmenter](https://crates.io/crates/icu_segmenter) 2.4.0**
    (2026-10-22; Unicode-3.0; ICU4X; 23.6M downloads; 7 releases in 12
    months): line, word, grapheme and sentence segmentation. The `lstm`
    and dictionary models (Thai, Lao, Khmer, Burmese) are optional. 4.47
    MB packaged; data through `icu_segmenter_data`. boa depends on it
    inside the wasm guest, so it isn't shared with the host.
  - **[unicode-linebreak](https://crates.io/crates/unicode-linebreak)
    0.1.5** (2023-07-25; Apache-2.0; tiny, no `unsafe`, Unicode 15.0; not
    pushed since 2023-07, so lagging). In the Linux app through
    cosmic-text.
  - [parley](https://crates.io/crates/parley) 0.12.0 /
    [fontique](https://crates.io/crates/fontique) 0.12.0 (2026-10-09;
    Apache-2.0 OR MIT; linebender): rich-text layout over its own font
    collection.
  - [cosmic-text](https://crates.io/crates/cosmic-text) 0.19.0
    (2026-04-22; MIT OR Apache-2.0; System76): GPUI's Linux text system.
- **Fit:** parley and cosmic-text lay out UI text over fonts they load
  (SFNT, through fontique or fontdb). They can't take an embedded Type 1,
  a CFF-only CID font or a Type3 font, and they don't model PDF text
  state. The layout has to be sheaf's, over the `Font` trait, with a
  segmenter for break opportunities.
- **Recommendation:** **build in-house on icu_segmenter** (maintained,
  current Unicode; the line segmenter without LSTM). unicode-linebreak is
  the lighter alternative. Not parley or cosmic-text as the engine.
  Medium.
