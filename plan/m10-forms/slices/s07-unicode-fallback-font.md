# M10 slice 7: A Unicode fallback font

Status: open · Part of [M10: Forms](../README.md)

**A Unicode fallback font** (proposal 11).
- **`Res::Font(Fallback)`:** a GID-preserving Noto Type0 subset (the
  rule the `Font` trait's docs set for it), late-bound per
  materialization or overlay: each output embeds the glyphs it draws,
  as Type 0 with `/W` widths, a CID-to-glyph map and a per-glyph
  `ToUnicode`. An Append adds a new subset object when new glyphs
  appear. A value mixes runs of the field's font and the fallback.
- **In lowering:** NFC, `MaxLen` and comb cells by grapheme cluster,
  and the refusal of a character no bundled font covers live in
  `SetFieldValues`' lowering (slice 9), so a refused value never
  commits. A notice for right-to-left text.
- **It ends the `'?'` interim** in marks and generators (M10.5
  proposal 22): mark text, stamps, headers, footers and watermarks
  draw what their fonts can't encode in the fallback, behind the same
  `Font` trait, and the "drawn as ?" notice goes.
- Its license in `packaging/licenses`. New dependencies, by name:
  skrifa (coverage, metrics and outlines; already sheaf-doc's) and
  subsetter (decision 11). **For Greg, at this slice:** subsetter
  always renumbers glyphs ("glyph IDs in fonts must be consecutive").
  With `/CIDToGIDMap /Identity`, it can't make the GID-preserving
  subset this slice's first bullet asks for (`content/font/mod.rs`,
  the `Font` trait: CID = GID). There are three ways
  (`plan/research/build-or-adopt/README.md` D1):
  - **Recommended (2026-10-25): take subsetter and map the old
    numbers to its new ones with a `/CIDToGIDMap` stream.** Each CID
    stays the glyph's number in the whole font, so codes still depend
    only on the font and the text, and only the font's map changes
    per subset. The map is at most 7,768 bytes before Flate for
    Noto's 3,884 glyphs. The `Font` docs change only their wording.
    subsetter's `GlyphRemapper::get` gives the map, and PDFium reads
    an embedded font's stream map (`CPDF_CIDFont::GlyphFromCharCode`).
    This slice's checks confirm the drawing.
  - Keep `/Identity` and build the subset on read-fonts (skera with
    `RETAIN_GIDS` if it grows). This was the memo's first
    recommendation, and is the fallback if the map stream draws wrong
    anywhere.
  - Drop the rule and take subsetter with `/Identity`: codes chosen
    per output, two passes in the emitter, and the `Font` docs
    changed.
- **Checks:** "Łukasz Nguyễn Ölçer" in an IRS field renders, and
  PDFium extracts it back; NFD input is stored as NFC; a CJK or emoji
  character gets the refusal; a value round-trips in `/V` as
  UTF-16BE; the subset holds only the glyphs used; a WinAnsi value
  doesn't embed the font; a note and a header saying "Łukasz" draw it,
  not `?`; an Append after a new glyph adds a subset and leaves the
  earlier one as it was. With a `/CIDToGIDMap` stream, PDFium,
  pdftoppm and pdf.js draw the subset alike, and the map's size after
  Flate is recorded.
