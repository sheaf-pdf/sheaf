# M10 proposal 10: **sheaf writes the appearance of every field it changes. It never

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**sheaf writes the appearance of every field it changes. It never
sets `NeedAppearances`, and doesn't fill through PDFium.**
- 91% of the corpus's text widgets (5,338 of 5,864) have no
  appearance until they're filled, and no file sets
  `NeedAppearances`. The generator is the core of filling.
- `NeedAppearances` hands the job to each viewer, and a viewer that
  doesn't run format scripts (sheaf's own PDFium build among them)
  draws the raw `/V`, not the formatted value.
- PDFium's form-filling calls (pdfium-render can set text and check
  box values) keep values in PDFium's own copy of the document,
  outside sheaf's model and history; they'd show raw values, and the
  result would have to be read back out of PDFium.
- Fields sheaf didn't change keep their appearances.
- A file that arrives with `NeedAppearances` (pdftk writes it with
  `need_appearances`, or for rich-text data) gets appearances for
  its valued fields on the first save that changes the form. The
  flag is cleared only when every one could be made.
- **On screen,** a field sheaf changed is drawn from the appearance
  sheaf will save: the realizer renders its page from an in-memory
  append section over the source (M10.5 §10), so the screen shows
  what the file will. While typing, sheaf's field editor draws it,
  and the view's `ViewInputs` hide the focused widget from the
  realization (proposal 11; slice 11).
- **Nothing touches the disk.** Edited pages render from bytes in
  memory, in the engine pool, off the UI thread (M10.5 §10.6); the
  generated cache is gone, and no field becomes a picture or a PNG,
  so no SSN or amount is left in plaintext under `~/.cache/sheaf`
  (M11 proposal 11). A test fills, saves to memory, closes, and
  finds no new file under the temp directory or sheaf's state and
  cache folders (the formjs compile cache's compiled guest aside).
- **Tests check with form drawing off.** sheaf's screen draws a value
  that has no appearance, since PDFium's `FFLDraw` reads `/V` (bug
  10). So the pixel checks render appearances only, which is what
  other viewers see.
