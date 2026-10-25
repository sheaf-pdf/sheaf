# M11 proposal 7: What's removed under a mark

Part of [M11: Security & redaction](../README.md) · [all proposals](README.md)

**What's removed under a mark.**
- **Text:** every glyph whose box, shrunk by 10% on each side, overlaps
  the mark. Shrinking keeps neighboring letters from being caught by
  ascenders and side bearings (MuPDF does the same).
  - Text in any render mode counts, invisible OCR text included.
  - Glyphs that survive are written out again with absolute positions
    (`Tm` per run), not TJ offsets.
  - A 2023 study (PETS) recovered removed text from the offsets of
    surviving glyphs in 11 tools, Acrobat among them. Absolute
    positions avoid that.
  - The gap a mark leaves still shows the removed text's length. That
    can't be avoided without reflowing, and the docs say so.
- **Images:**
  - An image wholly under a mark is removed.
  - A partly covered image is decoded, its pixels under the mark are
    replaced with the fill color, and it's encoded again.
    PDFium decodes, since it handles every filter. Bilevel images
    become CCITT G4 (`sheaf-image`), JPEGs become JPEG again, and the
    rest Flate.
  - An image shared with other pages is cloned first.
  - Soft masks and stencil masks are cleared under the mark. Inline
    images are handled the same way.
- **Line art:**
  - Paths wholly under a mark are removed.
  - Partly covered paths are clipped geometrically, keeping what's
    outside the mark. Curves are flattened only in the paths that get
    clipped, with a polygon-boolean crate such as `geo` or `i_overlay`
    (licenses to check).
  - A clip path can't be used instead, because it leaves the data in
    the file.
- **Shadings:** removed when wholly under a mark, otherwise kept. A
  known limit, as in MuPDF, and documented.
- **Form XObjects:** a copy is made for each use that a mark touches,
  and the copy is redacted. Other uses keep the original.
- **Annotations:** any annotation that overlaps a mark is removed:
  links, comments and sheaf marks. A widget that overlaps loses its
  widget and the value of its field.
- **Text kept beside the content:**
  - `/ActualText` and `/Alt` that span removed glyphs, in marked
    content and in the structure tree;
  - the words of OCR text layers under the mark;
  - `/ToUnicode` and `/Widths` entries for character codes no longer
    used anywhere in the document;
  - the `/Thumb` of a redacted page.
- **Whole pages:** Redact Pages… marks a page range. Each page becomes
  a blank page of the same size, with the fill and overlay text and no
  annotations.
