# M10 slice 12: Filling III: marks drawn by PDFium

Status: open · Part of [M10: Forms](../README.md)

**Filling III: marks drawn by PDFium** (proposal 10).
- **Changed fields already draw from their appearances** through the
  realizer (slice 6). This slice does the same for sheaf's marks:
  the screen switches to `MarkDraw::ByEngine`, and the GPUI mark
  painter (`annot_elements`), `stamp_preview.rs` and the tape
  pictures are deleted (`m10.5-decisions.md` D53).
- **FFLDraw** is decided: recommended, appearances only, once every
  changed field has an `/AP`.
- **Stamps made from a page, and the comment summary's page images,**
  draw changed fields the same way (proposal 7), from the page as the
  document shows it. Today Add from PDF Page… (`stamp_lib::one_page`)
  drops the page's `/Annots`, widgets included, and reads the file,
  not the unsaved state (M10.5 slice 19's Found), and it loads and
  clones the whole PDF to copy one page (M10.5 slice 20's); both are
  fixed here, against proposal 7.
- **Checks:** a changed field on screen matches the saved file
  rendered with appearances only; a mark on screen matches its saved
  appearance; a stamp made from a filled 1040 page, unsaved, and the
  comment summary's image of it, show the value; filling, saving to
  memory and closing leaves no new file under the temp directory or
  sheaf's state and cache folders (the formjs compile cache's
  compiled guest aside; proposal 10).
