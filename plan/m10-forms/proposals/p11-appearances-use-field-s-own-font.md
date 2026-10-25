# M10 proposal 11: **Appearances use the field's own font; a bundled Unicode font

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**Appearances use the field's own font; a bundled Unicode font
covers the rest.**
- The corpus is all simple single-byte fonts, Type 1 or TrueType,
  with no Type 0 fonts and no rich text. Encodings vary: 3,904
  fields use WinAnsi (IRS, NY, IL, Ohio's embedded TrueTypes); 1,806
  use Acrobat's default `/Helv`-style resources, a `/Differences`
  array in PDFDocEncoding's layout over the standard font's built-in
  encoding (10 of the 11 state forms); 79 (PA-40's Arial
  family) use MacRomanEncoding plus `/Differences`. 733 use
  ZapfDingbats' own encoding, and 114 name a font missing from
  `/DR`.

  | Fields | Font in `/DA` | In `/DR` |
  |---|---|---|
  | 2,942 | HelveticaLTStd-Bold or -Roman (IRS) | not embedded, with `/Widths` |
  | 110 | HelveticaLTStd-Bold (1040-ES) | missing |
  | 4 | ArialMT (1040-ES) | missing |
  | 1,251 | Helvetica | standard 14, no `/Widths` |
  | 733 | ZapfDingbats | standard 14 |
  | 498 | Courier-Bold | standard 14 |
  | 432 | CourierNew,Bold | embedded, whole font |
  | 358 | Arial | embedded, whole font |
  | 168 | TimesNewRomanPSMT | embedded, whole font |
  | 68 | OCRAStd | embedded, whole font |

- Widths come from the font's `/Widths`, or, for the standard 14,
  from built-in AFM tables looked up by glyph name: Helvetica, Times
  and Courier in four faces each, Symbol and ZapfDingbats. sheaf has
  Helvetica and Helvetica-Bold widths for ASCII and some punctuation
  (stamps.rs:207-254); the full AFM tables, and the other 12 faces,
  are new.
- Text is encoded in the field font's encoding: WinAnsi, MacRoman,
  or the built-in encoding with `/Differences`, each resolved by
  glyph name.
- A font named in `/DA` but missing from `/DR` falls back by name
  (Helvetica, Arial and `ArialMT` to Helvetica, Times to Times,
  Courier to Courier), written as a non-embedded font.
- Characters the field's font can't encode are drawn in a fallback
  font: Noto Sans, under the SIL OFL (kickoff), subset at
  save and embedded as Type 0 with a `ToUnicode` map. Nothing in the
  corpus needs it, but "?" in a client's name on a federal form
  isn't acceptable.
  - The OCR layer's Type 0 and CIDFontType2 dictionaries are a
    starting point (text_layer.rs:395-500). But it embeds one fixed
    glyphless font (every CID is glyph 1, `/DW` only, an identity
    `ToUnicode`), so subsetting, the CID-to-glyph map, `/W` widths
    and a per-glyph `ToUnicode` are new.
- **Text beyond simple Latin.** Values are normalized to NFC at
  commit (macOS input arrives as NFD). `MaxLen` and comb cells count
  grapheme clusters, and a comb cell holds one cluster. Right-to-left
  text and complex shaping are out, with a notice. A character no
  bundled font covers (CJK, emoji) is refused at commit: "This field
  can't hold 王 in a way other viewers will show." sheaf never
  writes `?` or `.notdef`. Installed fonts come later (Not in M10).
- **The editor's font.** The field editor (slice 11) paints with
  bundled faces whose widths match the field's: Liberation Sans,
  Serif and Mono (SIL OFL) for Helvetica and Arial, Times and
  Courier. It uses them for the characters the field's font encodes,
  and Noto Sans for exactly the characters the appearance will draw
  in the fallback. A field whose embedded font has no Liberation
  match (OCRAStd, 68 fields) is painted in that embedded font if
  GPUI can load it, else in Liberation Mono for a fixed-pitch font
  and Liberation Sans otherwise. So a typed value lines up with the
  appearance it saves as.
  `TextInput` can't: it paints one GPUI font and always aligns left
  (text_input.rs:545-548, :610), and 78% of the corpus's text
  widgets are right-aligned or centered.
- New dependencies for font metrics, subsetting, normalization and
  grapheme clusters: **skrifa** (read-fonts) for metrics, coverage
  and outlines, and **subsetter** for subsets (Greg, 2026-10-24;
  both MIT OR Apache-2.0), with `unicode-normalization` and
  `unicode-segmentation`. skrifa is in already: the content
  interpreter (M11 slice 3, its wrap-up) reads embedded font
  programs with skrifa 0.44 / read-fonts 0.41, the versions GPUI's
  Linux build compiles. subsetter renumbers glyphs. With
  `/CIDToGIDMap /Identity`, that conflicts with the glyph-ID rule
  slice 7 keeps; a `/CIDToGIDMap` stream from the old numbers to the
  new ones keeps the rule (`plan/research/build-or-adopt/README.md` entry 17 and D1,
  2026-10-25): settled at slice 7.
- This settles M9 decision 11 for forms. Stamps, headers and the
  comment summary can move to the same font later (Not in M10).
