# M10.5: Asks

Part of [M10.5: the edit API](README.md).


None of these blocks M10.5; each is wanted by the slice named.

- ~~**node and pdfjs-dist, for the ignored interop tier**~~ Answered
  2026-10-17: Greg installed node (v24.22.0, through nvm) and qpdf, and
  pdfjs-dist 6.4.299 sits outside the repo. Slice 7's interop tests ran
  with both; see its notes.
- **Proposals 8–10, at kickoff:** live page numbers (slice 17), strict
  removal (slice 10) and exports that keep encryption (slice 19). The
  plan assumes the recommendations.
- **Signed and certified fixtures.** Slice 10 picked P1, P2 and P3
  certified and approval-signed fixtures by script from the corpus's
  files with a signature (sheaf-ops `tests/append_corpus.rs`, `SIGNED`):
  `/P` 1 0000772, 0043679 (encrypted) and 0001207 (an approval
  signature's `/Lock /P 1`), `/P` 2 0000823 and 0101803 (encrypted), `/P`
  3 0008830 and 0338205 (encrypted), approval only 0000581 and 0023352
  (encrypted), all under `forms/safedocs/`. Greg's Acrobat check of
  "changes since signing" on them, after sheaf's Append of a comment
  (and Append anyway where it asks), and of the DocMDP, FieldMDP and UR3
  mapping (§9.3), is wanted before M12, not before M10.5 ships; until
  then the mapping is conservative (a ledger row). Slice 10's notes list
  what it asks about.
- **Reference files from Acrobat:** a file with a header/footer and a
  watermark made by Acrobat, for the `/PieceInfo` rule (slices 6, 16 and
  17), and whether Acrobat can update or remove sheaf's.
- **Word-tagged PDFs** with running headers and page numbers (slices 6,
  16 and 17). LibreOffice ones can be generated if `soffice` is
  available.
- **Acrobat round trips, by hand** (recorded in the slice notes): open a
  sheaf file and edit our header and watermark; open signed files sheaf
  appended to and read "changes since signing".
