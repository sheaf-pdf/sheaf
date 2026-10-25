# M5: What's in place

Part of [M5: Scan & OCR](README.md).


Slice 19 (`3e1b9f0`) rebuilt the paths this plan uses. Read from the code:

- **`Session`** (`crates/sheaf-ops/src/session.rs`): a document with no
  window, which every subcommand runs on. `Session::new_empty()` is a
  document of no pages made in memory, which a scan inserts its pages
  into; `commit`/`commit_as(actor, ops, label)` commit ops; `save(SaveTo,
  answers, p)` saves through `materialize::save_with` (`-o` a Rewrite,
  `-o --append`, `--in-place` as planned); `export` runs an `Export`. A
  save or export that stops at an ask is a `NeedsDecision` (exit 3).
- **Memory sources** (`crates/sheaf-doc/src/memory.rs`): blank pages,
  images as pages, and the empty document are small PDFs made in memory
  and interned as `Memory(kind)` blobs (`MemoryKind::{Library, Recovered,
  Blank, Images, Summary}`), which `InsertPages` brings in as a file's
  pages. The same inputs make the same bytes, so the same blob: an insert
  lowered again finds the pages it made. `PageSource` (sheaf-edit
  `op.rs`) is `File | Blank { size, count } | Images { images, fit }`,
  and `tx/resolve.rs` makes the memory source when the resolver builds.
- **`sheaf scan`** (`crates/sheaf-ops/src/scan.rs`): image files in,
  each cleaned (`sheaf_image::clean`, with the orientation votes and
  UVDoc), blank ones left out if asked, OCRed if asked, then one
  `InsertPages` of a `MemoryKind::Images` source into `Session::new_empty`
  (`made_pages`) and one `SetOcrText` per page as `Actor::Job(Ocr)`, then
  a Rewrite to `-o`. All pages are held until the end.
- **The app's Create from Scans** (`workspace.rs` `create_from_scans`,
  `scan_dialog.rs`): pick images, set options with a per-step preview,
  Create asks for a path, runs `sheaf_ops::scan`, then `open_paths` opens
  the written file (B11). Its choices persist in `state.json`
  (`State::scan`).
- **Combine's hand-off** (`workspace.rs` `show_made`, `DocView::adopt`):
  Combine saves its Session to the path the dialog asked for, then hands
  the same `Document` to a window (empty, or a new one); nothing is read
  again. `DocView.path` is a `PathBuf`: there is no document without a
  file.
- **Exports** (`crates/sheaf-doc/src/export.rs`): every way out besides
  a save is an `Export` (a version, transforms, a profile, an `Out`),
  written by `export::run`. `Export::extract(from, pages, out)` is a split
  part (`Transform::KeepPages`, headings kept, numbered as kept);
  `export::check` asks first without writing.
- **Split** (`crates/sheaf-ops/src/split.rs`): `SplitBy::{EveryN, Ranges,
  TopBookmarks, MaxBytes}`; parts are named `{stem}_{title}` with
  `file_safe` and `_2` for repeats, `{stem}_Start` for pages before the
  first bookmark; every part is checked before any is written.
- **The op vocabulary** (`crates/sheaf-edit/src/op.rs`): `InsertPages`,
  `DeletePages`, `AddBookmark { parent, at, title, target, … }`,
  `MoveBookmarks { with_pages }`, `SetOcrText`. Ops are serde, schemars,
  and plugins' and `sheaf edit`'s JSON.
- **Find Blank Pages** (`crates/sheaf-ops/src/blank.rs`,
  `crates/sheaf-app/src/blank_review.rs`): a planner over a snapshot reads
  each page as its views draw it (text first, then pixels at 60 dpi
  through `Views::render` in the background budget), then a review sheet
  with thumbnails and checkboxes commits one delete on the version it
  measured (`Base::At`, a stale page conflicts). This is the pattern for
  Separate.
- **Groups** (M10.5 §8.4, `GroupKind::{Ocr, PluginCall}`): a job's
  commits amend one version, one undo step, unless the user edits or
  saves in between; a job whose group the user undid is told
  (`GroupEnded`) and stops. `JobKind::{Ocr, Trim, Import, Insert}` name
  the actors.
- **Fonts for generated pages** (`sheaf-doc/src/content/font`): the
  standard 14 with exact widths; text a font can't encode draws `?` until
  M10 slice 7's fallback font.
- **Settings** (`sheaf_ops::prefs`): `settings.toml` is read, never
  written (`[identity]`, `[ocr]`, `[stamps]`, `[forms]`, `[plugins.*]`);
  dialogs' last choices are in `state.json`.
- **CI** builds and tests on Ubuntu, Windows and macOS runners, builds
  Tesseract and fetches the OCR data (so `--ocr` can be tested anywhere),
  and has dependency gates: `sheaf-image` stays pixels only, no OCR
  library is dynamically linked, macOS links only system libraries.
- **Plugins** (M10.5 slice 20, `sheaf:plugin@0.4.0`): reads of text and
  rendered pixels; edits as Op JSON, `Cap::Plugin` ops first. Inserting
  pages "comes later, with manifest permissions" (§14.3).
