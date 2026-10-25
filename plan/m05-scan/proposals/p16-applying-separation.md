# M5 proposal 16: Applying a separation

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**Applying a separation.** `separate::plan(found, pages)` turns
separators into **sections**: each starts at the first page after
its separator (and after the separator's back, below) and runs to the
next separator; pages before the first are a section titled "Start"
unless there are none. A trailer (`Place::Ends`, proposal 12) instead
ends the section that runs up to it, and takes its title from the
trailer's code. Then:
- **Bookmark:** one commit, labelled "Separate", on the version the
  search read (`Base::At`, as Find Blank Pages commits): an
  `AddBookmark` per section (level 1 at the top, `Sibling::Last`;
  level 2 under the last level 1), and one `DeletePages` of the
  separators that are dropped. One undo step.
- **Split:** one file per top-level section, each an extract
  (`Export::extract`) of its pages, separators left out, written to a
  folder as `sheaf split` writes parts: every part checked before any
  is written, an existing file replaced only when told. Level-2
  sections are bookmarks inside their part. The document isn't
  changed.
- **Both:** the commit, then the split of the version it made.
- **Names:** a part is named by its title alone (`W-2 Acme Corp.pdf`,
  `file_safe`, then `_2` for repeats), as PaperStream and CCH name
  them from the sheet; `{stem}_{title}` like Split Document's parts
  is the other choice, set in the dialog (remembered).
- **The sheet's back:** in a duplex scan, the back of a separator
  sheet is part of the sheet. While scanning, the scanner says which
  side a page is. In a PDF, a page right after a separator that
  `sheaf_image::blank` calls blank is taken as the back and goes with
  the sheet; the review shows it.
- **Two separators in a row** make an empty section: dropped, with a
  line in the review.
- **The review** (a sheet over the document, as Find Blank Pages'):
  a row per section with the separator's thumbnail, what it read
  ("QR: SHEAFSEP1 …", "Patch T", "Code 39: SEP-W2"), the title
  (editable), level, pages, and whether the separator is dropped.
  Apply makes the commit and the split. "N pages have no text"
  shows when a keyword rule couldn't read them.
- Alternative: split only, as most capture software does. A binder
  that stays one file with bookmarks is how CCH Scan and PDFlyer
  users work, so bookmarking stays the default for other software's
  separators, and sheaf's own sheets say which.
