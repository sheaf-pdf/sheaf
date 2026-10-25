# M10.5 §15: Private data after M10.5

Part of [M10.5: the design](README.md).


`sheaf_doc::private::KEYS` is enumerable. Sanitize and
`RemovePrivateData` read it. Everything else is standard keys.

| Key | Where | How Strip removes it |
|---|---|---|
| `/NM (sheaf:<id>)` | sheaf marks | rewritten to a neutral `/NM` |
| `/SheafAnnot` v2 | mark annotation dicts | dict key removed |
| `/SheafGenerators` | catalog JSON: specs only, no paths | catalog key removed |
| `/SheafBlock` | stream dict of each sheaf block and isolation stream | stream-dict key removed |
| `/SheafGen` | named `/Artifact` property list in `/Resources /Properties` | property-dict key removed |
| `/SheafOcrBlock` | named `/ShOCR` property list | property-dict key removed |
| legacy, read only: `/SheafStamp`, `SheafSettings`, `/SheafHF`, `/SheafHeaderFooter`, `/SheafPart`, `/SheafOcrLayer`, `/SheafTick*`; the resource names `/SheafOcr`, `/SheafFlat1`, `/SheafWM*` | inline property lists, stream dicts, `/Name`, resource dicts | `Filter::StripLegacy`, a lexical operand rewrite in the splice machinery (property-list keys dropped; resource names renamed to `/Sh…` in the page's `PageRes`), or a dict edit |

- A test fails if any other `Sheaf*` name appears in written document
  output.
- Stamp-library collection files (`/SheafStampText`, stamp_lib.rs) are
  the user's own library files. They are outside the edit API and
  outside this scan, and keep their format (proposal 29).
- Page-stamp appearance XObjects are named `PageRes`-style (`/Sh…`).
  `/SheafStamp` is read only.
