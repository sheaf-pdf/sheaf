# M10.5 §18: Invariants changed deliberately

Part of [M10.5: the design](README.md).


- **Coordinates and identity:**
  - Rotation is absolute.
  - Coordinates are PageSpace everywhere, including `/SheafAnnot` v2, Op
    JSON and the plugin API, which also gets view helpers.
  - A VersionId names one immutable state, and an amend replaces the
    version.
  - Commits validate what they read and overwrite, by value.
- **Saves:**
  - Text layers are blocks. Presence no longer means unsaved, and a
    saved layer can be removed.
  - A save re-expresses nothing. Dirty is `saved != cursor`, and the
    version being saved is sealed.
  - A file changed on disk is never overwritten without asking.
  - Save As of a signed document copies the file and appends.
  - Removal detection is strict and content-based, and a regenerated
    page number is the one exemption. On a signed document, a removal
    veto asks.
- **Annotations and fields:**
  - Others' comments are records, and `removed_comments` is retired.
  - A field whose last modeled widget is deleted is removed.
  - A stale `/SheafAnnot` opens as another tool's comment, and a
    duplicated `sheaf:` name gets a fresh id.
  - Copied widgets and annotations are untagged. Copies, and pages from
    other files' structure trees, lose `/StructParents`.
- **Generated content:**
  - A label range whose start page is deleted re-anchors to the next
    page of the range.
  - Header/footer page tokens are live in every output. Bates numbers
    and date and file tokens are frozen at apply and persisted per
    block. Watermarks stay upright.
  - Ours replaces Acrobat-added headers, footers and watermarks of the
    same kind. Authored pagination artifacts are never touched without
    `include_authored`.
  - Unencodable glyphs draw `'?'` with a notice, in marks and
    generators, until M10 slice 7.
- **Callers:**
  - Plugin calls are one undo step unless the user edits in between.
  - Combine opens its result in a new window without a reopen.
