# M5 slice 4: Separator sheets

Status: deferred · Part of [M5: Scan & OCR](../README.md)

**Separator sheets** (proposals 13 and 17).
- **Builds:** `PageSource::Separator { sheet }`,
  `MemoryKind::Separator`, `memory::separator`; Document > Insert
  Separator Sheets… (above top-level bookmarks with or without their
  children, above the selected pages, above typed pages; titles in an
  editable list); File > Create > Separator Sheets… (a new untitled
  document of sheets); `sheaf separator-sheets`.
- **Checks:**
  - The same sheet makes the same bytes; a sheet's code reads back
    from its own render at 150, 200 and 300 dpi, thresholded, at four
    turns.
  - Insert above every top-level bookmark of a forms-corpus file with
    bookmarks keeps each bookmark on its page, one undo step; an
    Append save and a Rewrite of it reopen alike.
  - A title with characters outside WinAnsi draws `?` and its code
    keeps the title exactly.
  - `sheaf edit` inserts a sheet from JSON; its schema lists it.
