# M10 proposal 7: **Usage rights go when the form changes; a replaced value forces a

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**Usage rights go when the form changes; a replaced value forces a
full rewrite; a signed file is never rewritten without asking.**
- `/Perms /UR3` (Reader usage rights) is in 31 of the 40 files: every
  IRS form, NJ and PA. Its signature covers the original bytes, so a
  rewrite breaks it. Today a rewrite keeps it (bug 7), and Reader
  would then say the extended features are no longer available (from
  memory; untested).
- So any save that changes the form, and any full rewrite, removes
  `/Perms /UR3`, and drops `/SigFlags` where no signature field needs
  it. In the corpus, SigFlags 2 is set on exactly those 31 files, and
  none has a signature field. A certification (`/Perms /DocMDP`) is
  detected and kept; only `/UR3` goes.
- **A replaced value forces a full rewrite,** as removing anything
  does since M9 slice 21 (`Snapshot::removes_content`, model.rs:918)
  and as README's Save model row says (README.md:152). Firms reuse a
  filled form as a template, and an incremental save would keep the
  last client's values in the earlier revision. Filling an empty
  field stays incremental.
- **So does the first change to a form whose earlier revisions hold
  values.** Files arrive with revisions (VA 760 has 22 `%%EOF`
  markers, every IRS form at least 3), and a template filled and cleared in
  Reader, which always appends, keeps the last client's values in
  them. When the form check (slice 1) finds earlier revisions whose
  field values differ from the current ones, the first save that
  changes the form is a full rewrite, with a notice.
- **Signed and certified files:** on a file with any signed field or
  a certification, a save that would rewrite asks: "Rewrite (breaks
  2 signatures)" or "Append (earlier values stay in the file)".
  - That covers a removal, a replaced value, dropping XFA, Flatten
    Fields and deleting a field.
  - It also covers M9's page delete, which today rewrites a signed
    file and breaks its signatures without a word (`removes_content`
    has no signature check). ROADMAP.md:293-294 left asking first to
    M12; this pulls it forward for any rewrite of a signed file.
  - Signed fields are read-only, and moving or deleting one asks
    first.
  - Signed fields arriving through Insert or Combine are flattened to
    their appearance, or kept with a notice: "The signature from
    fw9.pdf is no longer valid here."
  - Signing itself is M12.
- **Every way out follows these rules.** They live in save policy,
  `forms::rules::plan` (M10.5 §11.8), which every save plans with, and
  in the export preflight (M10.5 §12.1), which every export goes
  through, not in one save function:
  - Save, Save As and recovery sidecars, by both strategies: values
    kept, appearances made, `/XFA` and `/UR3` removed if the form
    changed.
  - Export Clean Copy keeps fields and values, under the same rules.
    M11's sanitize offers removing or flattening them.
  - Export Flattened Copy keeps fields unless "Form fields too" is on
    (proposal 18).
  - Extract Pages, Split and the CLI's Combine follow the page-tool
    rules (proposal 5).
  - Print, and so the print dialog's Save as PDF, flattens (proposal
    18).
  - Stamps made from a page, and the comment summary's page images,
    show the page's field values, unsaved ones included (slice 12).
  - One parametrized test runs every entry point on a filled hybrid
    1040, and the form check is clean on each (slice 15).
