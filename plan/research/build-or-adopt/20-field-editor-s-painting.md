# Build or adopt 20: The field editor's painting (slice 11)

Area: M10: Forms · Part of [build or adopt](README.md)

- **Need:** paint the value being typed so it lines up with the
  appearance it saves as: Liberation for what the field's font encodes,
  Noto for the fallback runs, and the embedded font when there's no
  Liberation match.
- **Where:** M10 slice 11 (`m10.md:1584-1590`); proposal 11
  (`:619-631`). D9.
- **Today's plan:** "A field whose embedded font has no Liberation match
  (OCRAStd, 68 fields) is painted in that embedded font if GPUI can load
  it, else in Liberation Mono for a fixed-pitch font and Liberation Sans
  otherwise" (`m10.md:624-627`).
- **Candidates:** GPUI itself:
  - `TextSystem::add_fonts(Vec<Cow<[u8]>>)` (`gpui-pre-0.3.9/src/text_system.rs:295`);
  - `Window::paint_glyph(origin, font_id, glyph_id, size, color)`,
    "only useful if you need to paint a single glyph that has already
    been shaped" (`window.rs:4821`); `GlyphId(pub u32)` can be
    constructed;
  - skrifa outlines, drawn as GPUI paths.
- **Fit:**
  - Shaping through GPUI (CoreText, DirectWrite, cosmic-text) can place
    glyphs differently from sheaf's layout. Painting sheaf's own glyph
    run (glyph IDs from skrifa on the same bundled bytes) with
    `paint_glyph` matches the appearance by construction.
  - **Security:** `add_fonts` with a PDF's embedded program hands
    untrusted bytes to CoreText (macOS) or DirectWrite (Windows), C and
    C++ font stacks with a history of font-parsing CVEs, and registers
    the font for the process's lifetime under the PDF's family name. On
    Linux it goes to fontdb and ttf-parser, which are Rust.
- **Recommendation:** **build on GPUI's `paint_glyph`**, with glyph runs
  from sheaf's layout, for the bundled fonts. Don't `add_fonts` a PDF's
  program: draw its skrifa outlines as paths, or use the Liberation
  fallback. Medium (`paint_glyph` with fonts from `add_fonts` is
  unverified on macOS and Windows).
