# M10 slice 6: Text field appearances

Status: open · Part of [M10: Forms](../README.md)

**Text field appearances** (proposals 10–11).
- **On the edit API:** a pure
  `appearance::text(field, widget, display, &FormDoc, font) -> FormRes`,
  written with `ContentWriter` and drawn in a `Font`
  (`sheaf-doc/src/content/`, M10.5 §13). The emitter calls it for
  widgets whose appearance digest changed, and so does the realizer,
  so the screen equals the file. Nothing else writes a widget's
  appearance.
- A `/DA` parser (font, size, `g`, `rg` or `k`), with a fuzz target.
  Encoders for each `/Encoding` form as `Font`s: WinAnsi and MacRoman
  on `content/font`'s encodings, and a font's built-in encoding with
  `/Differences`; the standard 14's AFM widths (`content/font`); both
  by glyph name. `/Widths` from `/DR`, its fonts' handles from the
  source's form facts (`Source::form`, the plan's `SourceForm`); the
  fallback by name, `ArialMT` included.
- Layout: `/Q`; auto size (`0 Tf`, 74 fields in 7 state forms);
  multiline wrapping by real widths; comb cells from `/MaxLen` (134
  fields), right-aligned too, as VA 760's 43 comb amounts are;
  `DoNotScroll`.
- `/MK` background and border colors, and border styles
  (`WidgetProps.border`: solid 390, underline 140, inset 66, dashed 9,
  beveled 5 in the corpus). `/MK /R` rotation through `/BBox [0 0 w
  h]` and `/Matrix` (none in the corpus, but Acrobat writes it).
- `/Tx BMC … EMC` with a clip, as Acrobat writes it. Gray, RGB and
  CMYK colors.
- **The overlay-or-pictures spike is settled:** an edited page renders
  from an in-memory append section over its source (M10.5 §10,
  proposal 16), so a changed field draws through the realizer, from
  the appearance the save writes, with no picture per field.
- **Checks:** PDFium pixel checks, appearances only, for each
  alignment, comb, multiline and auto size, upright and on a `/Rotate
  90` page; ours against PDFium's own appearance for the same value,
  within a tolerance; the IRS navy `0 0 0.502 rg`; "é € — “ ”" in an
  NJ `/Helv` field and a PA-40 `/Arial` field reads back; a realized
  page and the saved file draw a changed field alike. An ignored test
  renders the same outputs with `pdftoppm` and runs `qpdf --check` on
  each (proposal 21).
