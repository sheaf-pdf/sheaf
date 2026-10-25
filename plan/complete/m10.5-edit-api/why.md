# M10.5: Why

Part of [M10.5: the edit API](README.md).


**Three mechanisms change what a page shows, and the model sees none of
them as content.**
- **Overlays written at save.** Marks, OCR text layers, the
  header/footer and labels live in `DocState`, and `build_rewrite` or
  the incremental writer draws them into the file. Field values are read
  into the model but not written yet (M10 slice 9); the field tree's
  copies and renames are written by `forms::merge` in both writers.
- **Derived sources.** Crop, watermark/background and Remove
  Header/Footer write a whole new PDF per source to the plaintext cache,
  and `RebasePages` points pages at it. A save then routes the page
  object, `/Annots`, fields and structure parents back to the original
  page (`SourceInfo::derived`, M10 slice 3). The copy exists only to
  change content.
- **Export-only transforms.** Flatten, Clean Copy and print run lopdf
  transforms on the object graph after `build_rewrite`.
- Besides these three, blank pages, images-to-PDF, scans, stamps, tapes,
  the comment summary and Combine build new files outside the model.

What that costs today:
- History, plugins, the planned agent and the save rules can't tell a
  crop from a watermark: `describe.rs` prints "Change the content of …"
  for both (describe.rs:113). `removes_content` treats any page shown
  from another source as removed, so a crop forces a full rewrite though
  it removes nothing, while Remove Header/Footer really does remove.
- Every feature reinvents q/Q isolation and resource merging
  (text_layer.rs, stamps.rs, watermark.rs, save.rs).
- Two writers duplicate the page logic and drift apart: the
  header/footer is drawn only by the rewrite, and the outline is always
  rebuilt by one and only when dirty by the other.
- A derived copy costs O(file) per edit, a PDFium thread, and a
  plaintext file in the cache that is never deleted, form values
  included.
- The screen draws sheaf marks with GPUI, an unsaved header with a GPUI
  overlay, and the rest with PDFium, so the screen and the file can
  disagree.
- Edits aren't pure, so replay is fragile. Created ids aren't returned,
  so the app guesses them. Plans are applied to whatever state is
  current, with no base check. Found while planning has the details.

**What M11–M15 need that today's model can't express:**
- **M11 (security and redaction):** one content path to rewrite (a
  content interpreter on a lexer); the text kept beside content within
  reach (`/SheafAnnot`, bookmark titles, OCR streams); encryption kept
  on every save; no plaintext on disk; a leak check over what is
  written; and "every way out asks first" at one choke point.
- **M12 (signatures):** an incremental append whose bytes are
  reproducible and whose offsets are known; a change classifier for each
  signature permission (DocMDP, FieldMDP, UR3); the file's revisions.
- **M13 (content editing):** stable identity for pages, annotations and
  links across saves, and patches to single keys.
- **M14 (optimize, PDF/A, accessibility):** control over numbering,
  object streams, fonts and the structure tree.
- **M15 (batch) and the agent:** a versioned, serializable edit
  vocabulary, version-checked writes, and a way to revert one turn.

**Goals:**
- One document definition, `DocState`, from which one emitter derives
  every PDF object, on screen and on disk. Page content is part of it.
- One small public vocabulary of semantic ops, used by gestures,
  dialogs, jobs, the CLI and plugins, lowering to one primitive.
- Exact, checked undo with immutable versions, and commits that validate
  what they read.
- Saves as outputs: a planner decides Append or Rewrite from
  content-based effects before a byte is written, and deleting or
  replacing really removes.
- Interop with Acrobat, PDFium and pdf.js; encryption kept; standard PDF
  keys first; nothing on disk except the user's files, the recovery
  sidecar and a short-lived append journal.
- Speed at 7,000 pages: an untouched page costs about 200 bytes, a crop
  of every page commits in under 200 ms, and an incremental save of one
  mark on a 500 MB scan takes under 50 ms.
- Every existing feature re-expressed on the API, and the old paths
  deleted, within the milestone.
