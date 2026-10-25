# M11 proposal 8: Leak check across the document

Part of [M11: Security & redaction](../README.md) · [all proposals](README.md)

**Leak check across the document.** When a redacted copy is written,
each removed string (the text under every mark) is searched for in:
- every remaining page's text;
- bookmark titles and annotation contents, sheaf's own included (a
  Replace mark's struck text);
- metadata, form values, page labels, named destinations, link URIs
  and attachment names.

Hits are listed, and the export offers to remove them. A bookmark made
by "Bookmark matches" on an SSN is the obvious case.

**It runs over the output, not `DocState`** (M10.5 §20): a walk of the
emission (built values, and the source objects reachable from the
roots), with content and appearance strings decoded, plus the text
service over the written bytes. It covers Info and XMP, embedded-file
names, named destinations, link URIs and Launch targets, structure
`/Alt` and `/ActualText`, `/TU`, JavaScript, `/PieceInfo` and
`/SHEF_Annot`. A pass over `DocState` is only a fast pre-check.
