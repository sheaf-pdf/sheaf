# M5 proposal 2: Pages land where the user is

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**Pages land where the user is.**
- **Insert from Scanner** (Document > Insert Pages > From Scanner…,
  Acrobat's Insert from Scanner): pages go into the open document
  before the page chosen, or at the end, as they're scanned.
- **Create from Scanner** and Create from Scans (photos): ask 1.
  Recommended: a new window with an untitled document opens at once
  and pages stream into it; nothing is written until Save, which asks
  where (B11's fix: no file written and opened again). The other
  answer is Combine's: ask for the path first, scan in the dialog,
  save, hand the Session to a window.
- **Untitled documents** (if ask 1 says so) need, in the app:
  - `DocView.path` becomes optional (15 uses in `docview.rs`); the
    title says "Untitled scan" (numbered);
    - As built (slice 2): the field is private to `docview.rs`, and
      only the open, recovery and save code reads it. Its other
      readers (about 20 in `docview.rs`, plus export.rs, jobs.rs,
      comments_panel.rs, attachments_panel.rs and workspace.rs's
      `shows`) go through `path() -> Option<&Path>`, `folder()` and
      `stem()`. The plugin host's `DocContext.path` and Properties'
      `PropertiesContext.path` are `Option`s, and WIT's
      `document.path()` was already `option<string>`.
    - As built (slice 2): the field is a `Place`, a file or an
      `Untitled` (`untitled.rs`: its name, the folder Save As starts
      in, its images', and the claim on its name and sidecar). The
      name is "Untitled scan" and the first number from 1 that no view
      claims, in any sheaf process, and no sidecar has;
      `file_name()`, the title and `stem()` give it, and `path()` is
      None. Save is Save As there, suggesting "<name>.pdf", and a save
      makes the place the file. Its model goes by the same name
      (`Session::new_empty_named`, and a recovered one's), so a
      header's `<<file>>` says it until it's saved.
  - Save of an untitled document is Save As; closing one with pages
    asks, as a changed one does;
  - its crash-recovery sidecar lives in the state folder
    (`sheaf_ops::dirs::state_dir()/recovery/`) as a Full sidecar
    (the format already holds a whole document,
    `MemoryKind::Recovered`), and the next launch offers it, as
    recovery does beside a file today. A long scan is then safe from
    a crash after its first batch.
    - As built (slice 2): a third kind of format 2, `untitled`
      (`sheaf-recovery 2\nuntitled\n`, the folder Save As starts in,
      in hex, and the whole document, written as Full's is), since a
      Full sidecar's head names a base file and an untitled document
      has none: `recovery::write_untitled`, `find_untitled` (the
      folder's unheld ones, by name, read as far as their heads),
      `restore_untitled`, and `Document::never_saved` for the
      restored document, so it's dirty and its first save is a
      Rewrite. `find` beside a file takes no untitled sidecar. Each
      name is claimed (`recovery::Claim`: a lock file's lock, held by
      the view and each write in flight), so two sheaf processes, or
      a closed view's last write and a new document, never share a
      sidecar, and a launch never offers a live one.
    - As built (slice 2): the app starts its write as each batch
      lands, or as soon as the write in flight ends, not on
      autosave's clock (batches every 2 s would keep putting that
      off), and on autosave after other edits; it's removed when
      nothing is unsaved, with Don't Save, when the view closes with
      nothing unsaved, and once the document is saved to a file. A
      write in flight then removes what it wrote (a flag the view
      shares with it), even once the view is gone.
      A new document's name skips the names claimed and those
      sidecars there have. At launch the process that owns sheaf's
      state offers each no live view holds in its first window
      ("Recover the unsaved scan …?", Recover or Discard), not over a
      `--script` run's; a recovered one opens in an empty window or a
      new one, as a file opens (its pages first), its Save As
      starting in the folder its sidecar says.
- **The command line** lands in a `Session`, saved to `-o` at the end
  (as now).
