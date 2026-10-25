# Finished milestones, as the roadmap described them

Moved from ROADMAP.md when the plan was split (2026-10-25).

### M6: Plugins

Full design in [`plan/complete/m06-plugins.md`](m06-plugins.md).

- Data-driven command registry; menus and palette built from it.
- `DocView` events; `Document::apply_batch` as one undo step.
- WASM component host (wasmtime + WIT), manifests, discovery.
- Declarative UI: forms, notices, plugin-supplied panels.
- Permissions, plugin manager, SDK crate, authoring docs.

Later milestones use this host API where it fits. Batch actions (M15) are built
on it, and integrations such as CCH Workpaper Manager are left to plugins.

### M7: Viewing & output

**Done 2026-10-14** (Linux). Slice notes, the print decision and what's
left open (booklet / n-up, text "with layout", the Windows print backend
untested on Windows) are in [`plan/complete/m07-viewing.md`](m07-viewing.md).

Viewing:
- Single page, two-up (facing), and two-up with cover page modes.
- Rotate view (display only, not saved).
- Split view: the same document at two positions.
- Full-screen / read mode.
- Rulers, grid, guides.
- Page labels shown in the toolbar and thumbnails (display only; editing is M8).
- Attachments panel (list, open, save out).
- Open PDF Portfolios (read-only). When the catalog has a `/Collection`,
  open in portfolio view: the attachments panel lists the embedded files
  with their folders and metadata columns, the page view shows the cover
  sheet, embedded PDFs open in new windows, and any file can be saved out.
- Layers (OCG) panel: toggle visibility.
- Document Properties dialog (metadata, fonts, PDF version, security summary).
- Recent files.

Output:
- **Print**: printer selection, page ranges, scaling (fit/actual/shrink),
  annotations on/off, print selected bookmarks' pages, booklet/n-up ❓.
- Export pages to PNG / JPEG (resolution, range).
- Export to multi-page TIFF **(PDFlyer)**.
- Export text (plain; with layout ❓).

Slices: page modes + rotate view → split view → panels (attachments, layers,
properties) → print → image/TIFF/text export.

**Decided at kickoff (2026-10-14):** sheaf's own print dialog with a
preview, sending to CUPS (`lp`) on Linux/macOS and GDI on Windows; no
portal. Reasons in [`plan/complete/m07-viewing.md`](m07-viewing.md).

### M8: Page tools

- ✅ **Combine Files** (Acrobat's File → Create → Combine Files): build a new
  PDF from any number of PDFs and images (JPEG/PNG/TIFF, multi-page TIFF).
  - Add files or whole folders, or drop them in; reorder and remove in a
    list or thumbnail grid.
  - Expand a multi-page asset to choose a subset of its pages (pick
    thumbnails or type a range like `1-3, 7, 10-`).
  - Optional bookmark per source file (Acrobat's default), with each PDF's
    own bookmarks nested under it using the same merge rules as Insert PDF.
  - Image page size options: original size, or fit to Letter/A4. Image
    decoding reuses the M5 image import.
  - Command line: `sheaf combine -o out.pdf a.pdf:1-3 scan.tif b.png`.
  - Office files are not accepted (no Office conversion, see Not planned).
- ✅ Split document by page range, by top-level bookmark, or by file size.
- ✅ Insert blank page; insert pages from images (shares M5 image import).
- ✅ Replace pages (Acrobat's Replace Pages).
- ✅ Crop pages (CropBox), with the option to apply to a range.
- ✅ Page labels: edit numbering style, prefix, start.
- ✅ Headers / footers: add, update, remove (with date, page number, file name).
- ✅ Bates numbering.
- ✅ Watermarks and backgrounds (text or image, opacity, behind/in front).
- ✅ Bookmarks: create from a page range; colorize by level; auto-bookmark from
  a text pattern (reuses the Patterns engine).

Slices: Combine Files ✅ → split ✅ → insert blank/images + replace ✅ → crop ✅ →
page labels ✅ → headers/footers + Bates ✅ → watermark/background ✅ → bookmark
extras ✅. Done 2026-10-14; notes in [`plan/complete/m08-page-tools.md`](m08-page-tools.md).

Headers/footers, Bates and watermarks are written as tagged artifacts the way
Acrobat writes them, so Acrobat can update or remove them later and so can we.

### M9: Markup completion

Started 2026-10-14; the plan, with the decisions made at kickoff and the
bugs found while planning, is in [`plan/complete/m09-markup.md`](m09-markup.md).

- Infrastructure first (decided 2026-10-14):
  - **Single instance:** one sheaf process per user on each machine, and
    a second launch hands its files to it. Today two launches can open the
    same PDF in two processes, and each save discards the other's edits.
  - **Preferences split from state:** `settings.toml` (preferences, edited
    by hand, only read by sheaf) and `state.json` (window layout and
    last-used choices; disposable). Today every window saves its own copy
    of `settings.json` over the others'.
  - **Edit history:** every version keeps the commands that made it, in a
    tree so undone work isn't lost, and a History panel jumps to any
    version. It's also the base for merging conflicting edits later.
- Editing placed marks: resize and vertex handles, color, fill, opacity,
  line width and style, multi-select, copy and paste.
- Text selection and Copy, which text markup then acts on.
- Underline, strikethrough, squiggly, replace-text / insert-text marks.
- Polygon, polyline, cloud, callout (free text with leader).
- Custom stamps: from image or PDF page, in a shareable library (PDFlyer's
  user stamp export/import), with Recent and Favorites; dynamic stamps
  (date, time, name, initials) from a new identity entry in
  `settings.toml`.
- **(PDFlyer)** Set Status: bookmark color, page border and status stamp,
  and Reset Status. With dynamic stamps this replaces the planned
  "sign-off / review stamps", which PDFlyer doesn't have (real sign-off
  lives in ProSystem fx Engagement's binder).
- **(PDFlyer)** Connector groups: every connector with a label is one
  group, with Previous/Next and "pages with / without" navigation. Pairs
  keep working as groups of two.
- Comment status (accepted/rejected/cancelled/completed), checkmarks,
  replies, on third-party comments too; filters by type, author, status,
  color and page, sorting, hiding. PDFlyer's Stamp Summary and Note
  Summary are filter presets.
- Import / export comments as FDF and XFDF, or from another PDF; editing
  a third-party comment makes it a sheaf mark.
- Comment summary report (to PDF), and the list as CSV or text.
- File-attachment annotations.
- **(PDFlyer)** Calculator tape: sections and row numbers (`1-0`, `1-1`…,
  `1-T`, `GT`), a note column, editing a placed tape in place, rotating
  and resizing it.
- Calculator tape that picks up numbers by clicking page text. A sheaf
  extension: PDFlyer's tape numbers are typed.
- Measurement: distance, perimeter, area, with page scale and calibration.
  It was M9's cut line, the item that could have moved later.

Slices: single instance ✅ → preferences and state ✅ → groundwork and
bug fixes ✅ → edit history ✅ → editing marks ✅ → text selection ✅ → text
markup ✅ →
replies and status ✅ → comment filters ✅ → stamps ✅ → connector groups ✅ → Set
Status ✅ → tape format ✅ → tape from page text ✅ → shapes ✅ → file attachments ✅ →
summary and CSV ✅ → FDF/XFDF ✅ → measurement ✅ → plugin API 0.3 and docs ✅ →
deleting means deleted ✅ (added 2026-10-14 from M11 planning: deleted pages
stayed in saved files, and M11's redaction builds on the fix). Done 2026-10-15 (Linux);
notes in [`plan/complete/m09-markup.md`](m09-markup.md).

### M10.5: The edit API

Planned 2026-10-15, between M10 slices 5 and 6: M10 paused so that its
remaining slices are built on this API rather than revisited after it.
The plan, with the decisions made while planning, the proposals for
kickoff and the bugs found, is in
[`plan/complete/m10.5-edit-api/README.md`](m10.5-edit-api/README.md).

M11–M15 change PDF content far more surgically than anything so far, and
today three separate mechanisms change what a page shows (overlays drawn
at save, derived source files, export-only transforms), none of which
the model sees as content. M10.5 replaces them with one design:

- `DocState` is the document's definition, page content included, and
  one emitter derives every PDF object from it, on screen and on disk.
  The derived-source file cache goes.
- About 25 public semantic ops (a new `sheaf-edit` crate, serde and
  schemars) lower to one primitive, `Put { key, before, after }`.
  History stores those with their before-values, so replay is exact and
  checked.
- Effects are a pure function of each change, and drive dirty state,
  removal detection, signature impact and the save plan.
- Saves are outputs, never rebases: a planner picks Append or Rewrite
  before a byte is written, deleting or replacing really removes, and
  encryption is kept on every save.
- One frame below the page (unrotated user space), lazily read
  content-addressed sources, and an in-memory realizer for edited pages.
- Every existing feature moves onto it (crop, watermark, header/footer
  and Bates, OCR, flatten, the exports, Combine), plus `sheaf edit` JSON
  and plugin API 0.4 as the only world.

Slices: history stores what changed → spikes and gates → lazy sources and
the Store → one open → PageSpace → content toolkit and fonts → the
serializer → records → Rewrite from records → Append and the planner →
ops and Tx → one funnel → realizer → text service → crop → watermark →
header/footer and Bates → OCR → exports and `sheaf edit` → plugin API
0.4. M10 resumes after slice 20.
