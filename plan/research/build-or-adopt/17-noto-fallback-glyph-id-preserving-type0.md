# Build or adopt 17: The Noto fallback: a glyph-ID-preserving Type0 subset, late-bound per save

Area: M10: Forms · Part of [build or adopt](README.md)

- **Need:** embed only the Noto Sans glyphs an output draws, as Type0
  with `CIDToGIDMap /Identity`, `/W` and a per-glyph ToUnicode. An
  Append adds a new subset object when new glyphs appear.
- **Where:** M10 slice 7 (`m10.md:1454-1477`); M10.5 §13. D1.
- **Today's plan:**
  - "a GID-preserving Noto Type0 subset (the rule the `Font` trait's
    docs set for it), late-bound per materialization or overlay"
    (`m10.md:1455-1460`).
  - `content/font/mod.rs:35-38`: "Any font sheaf embeds is
    GID-preserving: a subset keeps each glyph's number from the whole
    font (CID = GID, `/CIDToGIDMap /Identity`), so the codes a string is
    drawn with depend only on the font and the text, and re-subsetting
    never changes them."
  - `m10.md:633` names "`subsetter`"; Greg leans to it.
  - M10.5 slice 2's `gen.py` already has a stdlib TrueType subsetter
    that "keeps glyph ids, empties the glyphs a section doesn't use
    (following composites)" (`m10.5-edit-api.md:6889-6899`).
- **Candidates:**
  - [subsetter](https://crates.io/crates/subsetter) 0.2.6 (2026-06-04;
    MIT OR Apache-2.0; 8.1M downloads; 3 releases in 12 months; Typst's,
    published by laurmaedje; used by krilla and so by typst's PDF export;
    `deny(unsafe_code)`; [GitHub](https://github.com/typst/subsetter)).
    The default `variable-fonts` feature pulls skrifa **0.42**,
    write-fonts 0.48 and kurbo; without it, only `rustc-hash`. **It
    always renumbers:** "glyph IDs in fonts must be consecutive. So if we
    only include the glyphs 68, 69, 70 … in the new font they will have
    the glyph IDs 1, 2, 3" (lib.rs docs); `GlyphRemapper` assigns IDs in
    insertion order. Input must be SFNT (`glyf` or `CFF `); CFF2 only
    with that feature; no bare CFF, no Type 1.
  - [skera](https://crates.io/crates/skera) 0.9.0 (2026-10-21; MIT OR
    Apache-2.0): fontations' Rust port of hb-subset. Young: first release
    2026-02-10, 14 releases, 35.9k downloads; used by fontations'
    incremental-font-transfer. Has `SUBSET_FLAGS_RETAIN_GIDS`. Subsets
    `glyf`/`loca`, `hmtx`, `cmap`, GSUB/GPOS, `gvar` and more; **no CFF
    table subsetter** in 0.9.0's `subset_table`. No `unsafe` in its
    source (rg), but no `forbid`. Depends on read-fonts 0.46, skrifa
    0.49, write-fonts 0.55 and hashbrown.
  - HarfBuzz hb-subset (has retain-gids): [hb-subset](https://crates.io/crates/hb-subset)
    0.3.0 (MIT; 2023-11-12; 12.8k downloads; stale; bundles HarfBuzz C++).
    [harfbuzz-sys](https://docs.rs/harfbuzz-sys/0.8.0/harfbuzz_sys/all.html)
    0.8.0 has `hb_shape*` but no `hb_subset_*`.
  - allsorts 0.17.0: `subset(provider, glyph_ids, …)` numbers the output
    by the order of `glyph_ids`, so it renumbers too. Apache-2.0.
  - "klippa" on crates.io is an [unrelated crate](https://crates.io/crates/klippa);
    fontations' subsetter is skera.
- **Fit:**
  - The keep-glyph-IDs rule is worth keeping: the emitter writes codes
    before it knows the subset, appearance digests stay independent of
    the save, and an Append's earlier subset stays valid.
  - subsetter breaks it with `/CIDToGIDMap /Identity`: adopting it that
    way means codes chosen per output (two passes in the emitter), and
    dropping the rule from M10.5's `Font` docs.
  - **With a `/CIDToGIDMap` stream it doesn't** (added 2026-10-25).
    Content keeps each CID as the glyph's number in the whole font, and
    the stream maps it to subsetter's number. `GlyphRemapper::get(old)`
    gives each entry, and `new_from_glyphs_sorted` makes the numbering
    ascending, whatever order the glyphs are met in. PDFium reads an
    embedded font's stream map in `CPDF_CIDFont::GlyphFromCharCode`, 2
    big-endian bytes per CID; a CID past the end draws no glyph. The map
    is at most 7,768 bytes before Flate for Noto, against about 31 KB of
    padded `loca` and `hmtx` under `/Identity`. Not yet drawn or
    measured after Flate. Without its default `variable-fonts` feature,
    subsetter adds only `rustc-hash`, not a second skrifa, and Noto Sans
    Regular is a static font.
  - skera fits Noto (a `glyf` font) with RETAIN_GIDS, but it's
    general-purpose: its output includes cmap and layout tables unless
    they're dropped.
  - The input is sheaf's own bundled font, not untrusted. A
    purpose-built subset is small: keep `glyf`/`loca` for the used glyphs
    and their composite closure; keep `hmtx`, `head`, `hhea` and `maxp`;
    `post` format 3; drop cmap, layout and hinting. It's deterministic
    across crate bumps, which matters because `FontRes` ids are digests
    of the bytes.
  - Measured: Noto Sans Regular, unhinted, is 431,364 bytes with 3,884
    glyphs. Keeping every glyph ID costs `loca` and `hmtx` about 15.5 KB
    each before Flate, mostly repeated entries (the compressed size is
    unverified).
- **Recommendation** (2026-10-25, after Greg named subsetter):
  **subsetter 0.2.6, without default features, behind a `/CIDToGIDMap`
  stream** (D1 option 1). If slice 7's checks find the map stream drawn
  wrong anywhere, fall back to the first recommendation: **build in-house
  on read-fonts** (a port of gen.py's algorithm), with **skera +
  `RETAIN_GIDS`** as the alternative if it grows. Not subsetter with
  `/Identity` while the rule stands. Medium.
