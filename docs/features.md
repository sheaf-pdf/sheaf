# Features

Legend: ✅ done · 🔨 in progress · 🎯 MVP · 📋 planned · ❓ undecided

## Viewing & navigation
| Feature | Status |
|---|---|
| Open / render PDF (incl. encrypted, forms, large scans) | ✅ (password prompt pending) |
| Continuous / single / facing page modes | ✅ View > Page Display: Single Page, Two-Up (each with Continuous), Show Cover Page |
| Zoom (fit page / fit width / custom), rotate view | ✅ rotate view is display only (Ctrl+Shift+Plus/Minus) |
| Thumbnails sidebar (lazy, virtualized) | ✅ |
| Bookmarks (outline) sidebar | ✅ |
| Text search (find in document) | ✅ progressive, in page order, through what each page shows: inserted, duplicated and cropped pages, and a header or recognized (OCR) text before it's saved; about 1–2 ms a page on 7,000-page files |
| Select and copy text | ✅ Select tool (V): drag over text, or double-click a word; Ctrl+C copies it. Recognized (OCR) text and a header work before they're saved. Press U with text selected to highlight it |
| Pattern search (regex: SSNs, EINs, amounts, dates) — separate tool from Find, shared engine with redaction | ✅ Patterns panel, Ctrl+Shift+F |
| User-defined patterns: add, save, and delete presets; several date formats built in | ✅ `patterns.json`; right-click a preset |
| Multiple documents, one window each | ✅ |
| Full screen / read mode | ✅ View > Full Screen (Ctrl+L; click or arrows turn pages, Esc leaves), Read Mode (Ctrl+H) |
| Page labels (display) | ✅ status bar and thumbnails; View > Page Navigation > Go to Page (Ctrl+Shift+N) takes a label or number |
| Rulers, grid, guides | ✅ View > Rulers (Ctrl+R), Grid (Ctrl+U); drag from a ruler for a guide |
| Split view (same document, two places) | ✅ View > Split (Ctrl+\\); F6 switches pane |
| Attachments panel | ✅ View > Panels > Attachments (Ctrl+Alt+6): open (PDFs in sheaf, others with the system program after a warning) or save out. **Attach File** (Shift+J from Stamp) puts a file on the page behind a push pin, paperclip, graph or tag; it's embedded when you save, and double-clicking the icon opens it. Flattening keeps the icon and drops the file, as Acrobat does |
| Open PDF Portfolios (browse and extract embedded files) | ✅ opens on its cover sheet with the Attachments panel: folders, metadata columns, open/save |
| Recent files | ✅ File > Open Recent (last 10, shared by every window) |
| Document Properties | ✅ File > Properties (Alt+Enter): description, security, fonts |
| Layers (OCG) panel | ✅ View > Panels > Layers (Ctrl+Alt+7): show or hide for viewing; never saved |
| History panel | ✅ View > Panels > History (Ctrl+Alt+8): every version kept, who made it (you, or a plugin by name) and when, and its edits in words, a run of one kind as one line ("Recognize text on 7000 pages"); an edit that removes something (pages, comments, fields, bookmarks, text) carries a Removes badge; click (or Up/Down) to go back to any version; edits undone and then replaced stay as a branch you can go into; an OCR run's or a plugin command's versions have "Undo all of …", which takes back every part of it, even when your edits in between split it ("… (continued)"), and keeps your edits |
| Configurable toolbar: any command or plugin command, docked to any edge or floating | ✅ View > Customize Toolbar…, or right-click a menu item, palette row or toolbar button; `toolbar.json` |

## Page organization
| Feature | Status |
|---|---|
| Reorder pages (drag in thumbnails) | ✅ |
| Rotate **individual** pages (PDFlyer-style, not whole doc) | ✅ |
| Delete / extract / duplicate pages | ✅ Document > Extract Pages… keeps marks and relevant bookmarks |
| Insert PDF at position (PDFlyer "Insert PDF") | ✅ Document > Insert Pages > From File… |
| Merge bookmarks on insert (parents merged, children appended) | ✅ |
| Combine Files: many PDFs and images into one, choosing pages from each | ✅ File > Create > Combine Files…: add files or a folder or drop them, reorder, a page range per file, a bookmark per file holding its own bookmarks, images at their size or fitted to Letter/A4. `sheaf combine -o out.pdf a.pdf:1-3 scan.tif b.png` |
| Split document (by page range, by bookmark, by size) | ✅ Document > Split Document…: every N pages, at each top-level bookmark (files named after them), by maximum file size, or by page ranges; unsaved changes included; Esc stops it. `sheaf split` |
| Insert blank page / from image | ✅ Document > Insert Pages > Blank Pages… (size of the current page or Letter/A4/Legal, how many, where) and From Images… (fitted to a page size or at their own) |
| Remove blank pages | ✅ Document > Find Blank Pages…: blank, nearly blank (a signature) and "intentionally left blank" pages found by content and pixels (margins, off-white paper, bleed-through and dust ignored); reviewed with thumbnails before one undoable delete. `sheaf remove-blanks` |
| Replace pages | ✅ Document > Replace Pages…: pages of this document with a run of another PDF's; bookmarks and comments on them stay |
| Insert from scanner | 📋 M5 |
| Crop pages | ✅ Document > Crop Pages…: margins (in, mm or pt, as the page is shown) or trim white margins, on the current, selected, all or a range of pages, previewed on the page as it will look; marks, form fields and OCR text stay on their content; saved as `/CropBox`, an incremental save writing only the cropped pages' dictionaries; all 7,000 pages of a large file crop in about 10 ms |
| Page labels (roman numerals, prefixes) | ✅ Document > Page Labels…: style, prefix and start for a run of pages; the pages after keep their labels; labels follow their pages through reorders and are written as `/PageLabels`; shown in the status bar and thumbnails, and Go to Page takes them |
| Headers / footers / Bates numbering | ✅ Document > Header & Footer… and Bates Numbering…: six places with page number, page count, page label, date, file name and Bates tokens; font, size, color, margins, page range; page numbers follow the pages through reorders and extracts, Bates numbers stay with their pages; previewed on the page as it will look and shown exactly as saved; written as tagged artifacts like Acrobat's, in place of Acrobat's or an older sheaf's on those pages, the author's own kept; found again when the file is reopened, and a reorder after a save written by an incremental save; Update and Remove, which takes others' off too (the author's own when asked) and says what it found; Bates numbers applied to 7,000 pages in under a millisecond |
| Watermarks / backgrounds | ✅ Document > Watermark… and Background…: text in any standard face, an image, or a PDF's page; opacity, rotation, nine positions, page range; in front of or behind the content, upright however the page is turned and on its crop; previewed on the page as it will look and shown exactly as saved; written as tagged artifacts like Acrobat's, in place of Acrobat's or an older sheaf's on those pages, the author's own kept; found again when the file is reopened, and added to a saved file by an incremental save; Update and Remove, which takes others' off too (the author's own when asked) and says what it found |

## Bookmarks (PDFlyer-heavy)
| Feature | Status |
|---|---|
| Create / rename / delete / nest bookmarks | ✅ |
| Move bookmark **with its pages** | ✅ Shift+drag |
| Colorize bookmarks | ✅ per bookmark; Document > Color Bookmarks by Level…; colors and bold/italic from the file (Acrobat's, PDFlyer's) are kept. PDFlyer colors by populated vs empty; that's 📋 ([Pending](../plan/pending.md#pdflyer-parity-gaps)) |
| **Set Status** (PDFlyer) | ✅ Document > Set Status, or right-click a bookmark: colors and styles it, borders each of its pages, and stamps them with the status and your initials and date; Set Status Again repeats the last one, Reset Status takes it off (the stamps too, if you say so). One undo step each |
| Bookmark from page range | ✅ Document > Bookmarks from Pages…: one per page, titled from a template (`Page <<label>>`), optionally under a new parent |
| Bookmark pattern matches | ✅ Patterns panel > Bookmark matches: one per match, pointing at it |
| Target sheets / auto-bookmark from scan separators | ❓ |

## Markup & annotation
| Feature | Status |
|---|---|
| Sticky notes, highlight, underline, strikethrough | ✅ also squiggly. Drag across text (or select it, then press the tool's key: U highlights); Shift+U cycles highlight, underline, strikethrough, squiggly. Each tool keeps its own color, as Acrobat's do: yellow notes and highlights, red strikethroughs, drawings and tickmarks, blue text edits, until you pick another for it; with the Select tool, the swatches recolor the selected marks |
| Text edits: replace or insert text (Acrobat's) | ✅ Text Edits (E): drag across text and type the replacement, or click between characters and type what to insert. Written as Acrobat writes them (a Caret, with the StrikeOut grouped under it); the Comments panel reads Replace “x” with “y” |
| Free text, shapes (rect, ellipse, line, arrow, polygon) | ✅ also polyline, cloud (Q: a cloudy polygon; any rectangle, ellipse or polygon can be made cloudy) and callout (a text box with a leader; Shift+X from the text box). Polygons: click each corner, double-click or Enter finishes, Backspace takes one off. Shift+D cycles the drawing tools. Every handle drags: corners, vertices, a callout's box, knee and point |
| Ink / freehand | ✅ |
| Edit placed marks: move, resize, restyle | ✅ Drag to move (Ctrl+drag under any tool), handles to resize (Shift keeps the shape, or snaps a line to 45°), Shift+click to select several. A properties bar sets color, fill, opacity, line width, dashes, arrowheads at either end and text size; each change is one undo step |
| Copy and paste marks | ✅ Ctrl+C, Ctrl+V (offset), Ctrl+Alt+V (in place), across documents too |
| Stamps (built-in + custom) | ✅ tickmark library; a stamp library of your own (Stamps panel, Ctrl+Alt+9; Stamp tool J): stamps from images (transparency kept) or PDF pages, in collections, with rename and remove; Recent and Favorites; dynamic stamps (Approved, Reviewed, Received, Prepared) with your initials or name and the date, and dynamic text over a stamp of your own. Collections export and import as PDFs, the way Acrobat keeps custom stamps |
| **Tickmarks** (PDFlyer): quick-stamp library, keyboard-driven | ✅ |
| **Connectors** (PDFlyer): linked markers that jump between pages | ✅ label groups as in PDFlyer (1–50, A–ZZ): the tool keeps its label, so each click adds to the group (Shift+I starts a new one); clicking one goes to the next. Saved as Link annotations that walk the group in page order, so they jump in Acrobat too. Previous/Next ([ and ], or right-click) and "pages with / without" work for tickmarks and stamps as well |
| **Calculator tape** (PDFlyer): 10-key calc, tape placed as annotation | ✅ Calculator panel (Ctrl+Alt+4), in PDFlyer's format: sections numbered `1-1`, `1-2`… `1-T`, titles, subtotal (Space) and total (Enter), grand total (Shift+Enter), notes on rows (double-click; Ctrl+T dates them), memory (G H J K L), 00 and 000, change sign (S), decimals (D), insert and delete rows with the totals worked out again. Double-click a placed tape to edit it; turn it, resize it, hide its numbers or titles, and color its paper. **Pick from page** (a sheaf extension): click a number on the page to add it, or Shift+drag over a box for all of them, with a tickmark beside each if you like; it reads recognized (OCR) text too |
| Comments list / review panel, status, replies | ✅ Comments panel (Ctrl+Alt+5) lists ours and third-party, in threads. Reply (R; Ctrl+Enter sends), set a status (right-click: None, Accepted, Rejected, Cancelled, Completed, with its history), checkmark (Shift+K), on anyone's comments. Written as Acrobat writes them, and Acrobat's replies to our marks stay attached across saves. Filter by type, author, status, color, checkmark and pages (what's filtered out is hidden on the page too; Show All undoes it), sort by page, author, date, type or color, and PDFlyer's Stamp Summary and Note Summary as presets. Other tools' comments can be deleted (one undo step; gone from the file at save). View > Show Comments hides them all |
| Comment summary and export | ✅ File > Comments, or the panel's Export menu: what the panel lists, filtered and sorted as it is, as a summary PDF (the comments numbered and threaded; or each page beside its list, with the numbers on the page), a CSV (columns chosen in the Export menu: number, page, page label, type, author, dates, status, checked, color, text, reply-to) or text. `sheaf comments` |
| Import / export comments (FDF, XFDF) | ✅ File > Comments > Import… takes FDF, XFDF or another PDF's comments: one that's here already (by its `/NM`) is updated, the rest added, as one undo step, and kinds sheaf can't hold are listed. Export XFDF or FDF of what the panel lists, or (panel's Export menu) what's selected. `sheaf comments import`/`export` |
| Edit other tools' comments | ✅ Another tool's note, highlight, shape, line, ink, text box, callout, text edit or stamp becomes a sheaf mark when it's edited: drag it, double-click it (or F2 in the panel) to edit its text, or right-click > Make Editable in the Comments panel. It keeps its name, so its replies stay with it, and a save puts it where it was |
| Flatten annotations | ✅ File > Export Flattened Copy…: every comment, mark and form field shown drawn into its page as viewers show it (hidden ones left out), so it looks the same anywhere and can't be edited; links kept. `flatten-annotations` in `sheaf edit` |
| Export Clean Copy (strip annots, notes, tapes, stamps, attachments) | ✅ File > Export Clean Copy; keeps links, form fields, bookmarks, and none of sheaf's own data (its names for marks, an older sheaf's watermark settings) |
| Measurement tools (distance, area, perimeter, scale) | ✅ Tools > Measure: Distance (Ctrl+Shift+D, a drag), Perimeter (Ctrl+Shift+P) and Area (Ctrl+Shift+A, corner by corner, as Polyline and Polygon). The scale bar takes a scale as typed ("1 in = 10 ft", "1/8 in = 1 ft", "1:100") and the decimals; Calibrate (Ctrl+Shift+C) is a line of known length, then its real length. The status bar shows the measurement while drawing; Shift keeps to 45°, and points snap to the grid and guides when they show. Written as Acrobat's dimension annotations (`/Measure`), with the value on the mark; the comment CSV has Measurement and Scale columns |

## Content editing
| Feature | Status |
|---|---|
| Edit text in place (reflow within block) | 📋 |
| Edit / replace / move images | 📋 |
| Add text / image objects | 📋 |
| Link editing | 📋 |
| Redaction (true content removal, pattern search) | 📋 |
| Header/footer editing | 📋 |

## Forms
| Feature | Status |
|---|---|
| Fill AcroForms | 📋 |
| Create / edit form fields | 📋 |
| XFA | ❌ not planned |

## Conversion & export
| Feature | Status |
|---|---|
| Export pages to PNG / JPEG | ✅ File > Export Pages (Ctrl+Shift+E): resolution, JPEG quality, page range, marks included or not; Esc stops a running export. Also `sheaf export` on the command line |
| Export to multi-page TIFF (PDFlyer) | ✅ File > Export Pages > TIFF: one file, black-and-white (CCITT G4), grayscale or color |
| Create PDF from images | ✅ File > Create > From Scans/Photos…: phone photos cleaned up (page found and straightened, upright, deskewed, shadows and color casts evened out) or a scanner's or copier's pages (upright and deskewed), each kind's choices remembered apart, color/gray/black-and-white, Letter/A4/as photographed, optional OCR, blank pages left out if asked (the preview dims them), with a preview of each step; the pages stream into a new untitled document as they're cleaned up, and nothing is written until it's saved. `sheaf scan` and `sheaf images-to-pdf` on the command line |
| OCR (searchable text layer) | ✅ Document > Recognize Text (OCR)… (Ctrl+Shift+T): Tesseract or PaddleOCR, whole binder / current / selected / range, skips pages that already have text, progress + Esc to stop, one undo step; Find and Patterns see the text before the first save. Document > Remove Recognized Text… takes it off the selected pages or every page, saved or not (one undo step); `clear-ocr-text` in `sheaf edit` |
| Export text | ✅ File > Export Pages > Text: one UTF-8 file, a form feed between pages (unsaved OCR text included) |
| Print | ✅ File > Print (Ctrl+P): own dialog with sheet preview; printer, copies, pages (all/current/selected/bookmark/range), Fit/Actual/Shrink, orientation, paper, two-sided, comments on/off; CUPS on Linux/macOS, GDI on Windows; Save as PDF |
| PDF/A conversion | ❓ |
| Office ↔ PDF | ❌ not planned (use LibreOffice) |

## Security
| Feature | Status |
|---|---|
| Open password-protected PDFs | ✅ prompt on open; saves, Save As and exports stay encrypted with the same password (printing doesn't) |
| Set / remove passwords, permissions | 📋 |
| Digital signatures (sign / verify) | 📋 |
| Certificate encryption | ❓ |

## Other Acrobat Pro features
| Feature | Status |
|---|---|
| Compare documents | 📋 |
| Optimize / reduce file size | 📋 |
| Preflight / accessibility checker | ❓ |
| PDF/UA validation and accessibility review | ❓ |
| sheaf itself with a screen reader and the keyboard | 📋 |
| Batch actions / scripting | 📋 (Lua or Rhai, not JS) |
| Create Portfolios | ❌ not planned (opening them is planned) |
