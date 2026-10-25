# M5 proposal 17: Insert Separator Sheets

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**Insert Separator Sheets.** `InsertPages { from:
PageSource::Separator { sheet }, before: Some(page) }`, one per
place, one commit ("Insert separator sheets"), one undo step.
- **Where (Document > Insert Separator Sheets…):** above each
  top-level bookmark, titled with its title (the default); with its
  children too, as level 2 sheets; above the selected pages; or above
  pages typed in ("3, 10, 25"), titled "Document 1", "Document 2"…;
  each title editable in a list before inserting.
- **What they say:** split, bookmark or both, and drop (default) or
  keep.
- The bookmarks keep pointing at their pages, so after a print and a
  rescan, Separate rebuilds the same bookmarks, or the same files.
- **Removing them:** undo, before saving. Afterwards, Separate with
  neither bookmarks nor a split ("Remove separators only") takes them
  out, as PDFlyer's Remove does.
- **A stack to print:** File > Create > Separator Sheets… makes a new
  document of sheets from a list of titles (one a line, or none for a
  plain "Split here" sheet, with copies), to print and keep by the
  scanner. `sheaf separator-sheets --titles … -o sheets.pdf` too.
- Alternative: ship fixed printable sheets as PDFs, as PaperStream
  and Kodak do. Sheets made from the binder's own bookmarks are the
  point of Insert Target Sheets, and a list of titles covers the
  generic stack.
