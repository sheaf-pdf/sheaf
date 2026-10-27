# Backlog: bugs and features found along the way

Status: open log (started 2026-10-21). Bugs and feature ideas that turn
up during milestone work, written down so they aren't lost. Nothing here
is scheduled. We don't fix these in the middle of a milestone phase;
they're triaged between phases.

## How to use it

- **Add, don't fix.** When something comes up that isn't part of the
  current slice, write an entry and keep going. Agents working a slice
  do the same.
- **One file per entry,** `B<n>.md` or `F<n>.md` in this folder, from the
  templates below, with a `Status: open` line under the title; and one
  row in the tables below. `just plan-check` checks that the files and
  the rows agree. IDs are never reused: take the next number after every
  ID here and in [the closed entries](../complete/backlog-closed.md).
- **Check for overlap first.** A feature already in
  [the roadmap](../../ROADMAP.md) (a milestone, [the parity
  checklist](../parity.md), [Pending](../pending.md) or [Not
  planned](../not-planned.md)) or in a plan file gets a line there, or a
  link here, not a duplicate.
- **Triage between phases.** A closed entry moves to [the closed
  entries](../complete/backlog-closed.md), with a `**Closed:**` line
  saying where it went, and its file and row here go:
  - fixed, with the commit or PR;
  - a slice in a milestone plan;
  - a feature into [Pending](../pending.md), where every feature
    proposal starts;
  - its own memo in `plan/topics/`, if it needs research;
  - won't fix or not planned, with a reason.

### Bug entry

```markdown
# B<n>: <short title>

Status: open

- **Found:** <date>, <who>, during <milestone/slice>
- **Where:** <crate, file:line, command or view>
- **Severity:** data loss | wrong output | crash | annoyance
- **What happens:** <observed>
- **Expected:** <what should happen>
- **Repro:** <steps, corpus file, or test>
- **Notes:** <suspected cause, related entries or memos>
```

### Feature entry

```markdown
# F<n>: <short title>

Status: open

- **Found:** <date>, <who>, during <milestone/slice>
- **What:** <the feature>
- **Why:** <who needs it, what it unblocks>
- **Overlap:** <roadmap items or memos it touches, or none found>
- **Notes:** <prior art, rough size, open questions>
```

## Bugs

| ID | Title | Kind | Found |
|---|---|---|---|
| [B2](B2.md) | Scrolling over a dialog also scrolls the page behind it | annoyance | 2026-10-21, M10.5 |
| [B4](B4.md) | The realizer's predicate and the emission disagree on a page that can't be read | annoyance | 2026-10-22, M10.5 slice 18. Greg merged #26 with it logged… |
| [B9](B9.md) | A radio group keeps its value when its chosen button's page is removed | wrong output | 2026-10-22, M10.5 slice 19's export corpus tier |
| [B10](B10.md) | After a Combine, the "XFA part holds values" bar can stay up | annoyance | 2026-10-22, M10.5 slice 19 |
| [B14](B14.md) | A TIFF's Orientation tag is ignored by images-to-pdf, Combine and image inserts | wrong output | 2026-10-23, M5 slice 1 |
| [B15](B15.md) | Some TIFF kinds are refused: CMYK16, float RGB/RGBA, planar | annoyance | 2026-10-23, M5 slice 1 |
| [B16](B16.md) | The command line hangs if its work panics | crash | 2026-10-23, M5 slice 1's review |
| [B17](B17.md) | The Combine dialog decodes every image just to count its pages | annoyance | 2026-10-23, M5 slice 1 |
| [B18](B18.md) | images-to-pdf holds every gray and color page's pixels until it writes | annoyance | 2026-10-23, M5 slice 1 |
| [B19](B19.md) | A bookmark selection isn't tracked, so a re-lowered edit can land on other pages | wrong output | 2026-10-23, M10.5 slice 20 |
| [B20](B20.md) | A plugin's region events wait forever after a window closes mid-call | annoyance | 2026-10-23, M10.5 slice 20 |
| [B23](B23.md) | A fuzz test's wall-clock budget fails when the machine is busy | annoyance | 2026-10-23, the instance-handoff migration's workspace run |
| [B24](B24.md) | G4 encoding truncates images wider than 65,535 pixels | wrong output | 2026-10-23, the build-or-adopt audit |
| [B25](B25.md) | Fax TIFFs (CCITT Group 3, Modified Huffman) are refused | annoyance | 2026-10-23, the build-or-adopt audit |
| [B26](B26.md) | The Flate backend depends on which crates are in the build | annoyance | 2026-10-23, the build-or-adopt audit |
| [B27](B27.md) | Nothing checks dependencies for advisories or licenses | annoyance | 2026-10-23, the build-or-adopt audit |
| [B28](B28.md) | Printing may not work in the Flatpak build | annoyance | 2026-10-23, the build-or-adopt audit |
| [B29](B29.md) | Search misses ligatures, compatibility forms and NFD text | wrong output | 2026-10-23, the build-or-adopt audit |
| [B30](B30.md) | Images to PDF and scan outputs have no /Producer or /CreationDate | annoyance | 2026-10-23, the backlog-fixes PR |
| [B31](B31.md) | sheaf and pyHanko count a file's signatures differently | other | 2026-10-23, the B5 PR's tier over `signatures/` |
| [B32](B32.md) | An untitled document's recovery sidecar is never encrypted | privacy | 2026-10-23, M5 slice 2 |
| [B33](B33.md) | Opening a file in the recovery folder can delete an untitled sidecar | annoyance | 2026-10-23, M5 slice 2 |
| [B34](B34.md) | A file's recovery sidecar can outlive an undo to its saved version | annoyance | 2026-10-23, M5 slice 2 |
| [B35](B35.md) | Opening a file reads its whole recovery sidecar and holds it until answered | annoyance | 2026-10-23, M5 slice 2 |
| [B36](B36.md) | sheaf-doc's tests leave scratch folders in /tmp on every run | annoyance | 2026-10-23, M5 slice 2 |
| [B37](B37.md) | A stray doc comment above `open_recent` in workspace.rs | cosmetic | 2026-10-23, M5 slice 2 |
| [B38](B38.md) | Corpus manifests are read without their CSV quoting | wrong output | 2026-10-23, between milestones, while building a fetcher… |
| [B39](B39.md) | imageproc's Otsu threshold overflows on large one-gray images | wrong output | 2026-10-23, M5 slice 3 |
| [B40](B40.md) | The Arlington model gives AnnotSquare `/CA` from PDF 2.0 | annoyance | 2026-10-24, the Arlington checks |
| [B41](B41.md) | A connector is a Link annotation with every mark's keys | wrong output | 2026-10-24, the Arlington checks |
| [B42](B42.md) | sheaf's marks never raise the file's PDF version | wrong output | 2026-10-24, the Arlington checks |
| [B43](B43.md) | The standard 14 fonts are written as `/BaseFont` alone | wrong output | 2026-10-24, the Arlington checks |
| [B44](B44.md) | An Append's new objects can take numbers the file refers to but doesn't have | wrong output | 2026-10-24, the Arlington checks |
| [B46](B46.md) | The Arlington carried rule matches a merged object's finding against every object of the sources | other | 2026-10-24, the Arlington checks' review |
| [B47](B47.md) | A form that draws itself is refused at once; PDFium draws it 40 deep | wrong output | 2026-10-23, M11 slice 3 |
| [B48](B48.md) | An inline image's end depends on what follows it when `/L` runs past the content | wrong output | 2026-10-23, M11 slice 3 |
| [B49](B49.md) | Text and inline images in a form may draw otherwise once written again | wrong output | 2026-10-23, M11 slice 3 |
| [B50](B50.md) | An XObject lopdf can't read stops the pages that draw it (0221018's `14 0 R`) | annoyance | 2026-10-23, M11 slice 3 |
| [B51](B51.md) | An embedded CMap with no mappings may give PDFium each code as its CID | wrong output | 2026-10-23, M11 slice 3 |
| [B52](B52.md) | Codes a `/ToUnicode` lacks don't fall back to their CID's text | wrong output | 2026-10-24, the content interpreter's wrap-up |
| [B53](B53.md) | A font without `/Widths` takes no widths from its embedded program | wrong output | 2026-10-24, the content interpreter's wrap-up |
| [B54](B54.md) | Tricky TrueType fonts are known by name only | wrong output | 2026-10-24, the content interpreter's wrap-up |
| [B55](B55.md) | An embedded CMap over a predefined base guesses every code | annoyance | 2026-10-24, the content interpreter's wrap-up |
| [B56](B56.md) | retired.rs misses a retired type named through a re-export or as `Type::method` | annoyance | 2026-10-24, the content interpreter's wrap-up ,… |
| [B57](B57.md) | `content::marked` and the interpreter disagree on a `BDC` naming a list `/Properties` lacks | other | 2026-10-23, the content interpreter ; logged at its… |
| [B58](B58.md) | Predefined CMaps PDFium knows but no Adobe bundle has stay guessed | annoyance | 2026-10-24, the content interpreter's wrap-up |
| [B59](B59.md) | A Type3 glyph whose `d1` box has no area takes the font's `/FontBBox` | annoyance | 2026-10-23, the content interpreter ; logged at its… |
| [B60](B60.md) | Launch to first pixels takes 1.2–2 s on Windows | annoyance | M0 |
| [B61](B61.md) | After a save, Properties still describes the file as opened | wrong output | 2026-10-26, object streams slice 1 |
| [B62](B62.md) | Packing on Append | feature | 2026-10-26, object streams slice 2 |
| [B63](B63.md) | pdf-lint shows a long UTF-16 string two ways in an encrypted file | annoyance | 2026-10-26, the pdf-lint item's slice 1 |
| [B64](B64.md) | CI's paths filter still names todos/ and skips only the root's Markdown | annoyance | 2026-10-26, the pdf-lint item's slice 1 |

## Features

| ID | Title | Kind | Found |
|---|---|---|---|
| [F1](F1.md) | Basic calculator | feature | 2026-10-21, M10.5 |
| [F2](F2.md) | Percent key on the 10-key | feature | 2026-10-21, Claude, while checking the same report |
| [F3](F3.md) | Put a calculator result on the page as text | feature | 2026-10-21, M10.5 |
| [F4](F4.md) | The calculator as a popup, not a panel | feature | 2026-10-21, M10.5 |
| [F5](F5.md) | Plugins ask for a command, not a panel; sheaf picks the view | feature | 2026-10-21, M10.5 |
| [F6](F6.md) | Make Go to Page easy to find | feature | 2026-10-21, M10.5, who asked for "goto page#" as useful in… |
| [F7](F7.md) | Scrollbars on everything that scrolls | feature | 2026-10-21, M10.5 |
| [F9](F9.md) | Test against pdf-corpus's signatures/ and conformance/ collections | feature | 2026-10-22, M10.5 slice 18 |
| [F10](F10.md) | Redo OCR: replace another tool's text layer | feature | 2026-10-22, M10.5 |
| [F11](F11.md) | Plugin memory and time limits in settings.toml | feature | 2026-10-22, M10.5 |
| [F12](F12.md) | A qpdf plugin | feature | 2026-10-22, M10.5 |
| [F15](F15.md) | A Rewrite that doesn't load the whole file (the flatten export's memory gate) | feature | 2026-10-22, M10.5 slice 19 |
| [F16](F16.md) | Turn QR codes into links | feature | 2026-10-23, M10.5 |
| [F17](F17.md) | hayro-interpret as a second corpus oracle beside PDFium | feature | 2026-10-24, the content interpreter's wrap-up |
| [F18](F18.md) | Notices for the data and code sheaf ships | feature | 2026-10-23, the build-or-adopt audit |
| [F19](F19.md) | Wipe file keys, not only passwords | feature | 2026-10-23, the build-or-adopt audit |
| [F20](F20.md) | Upstream: cms `CertificateChoices` alternatives for attribute certificates | upstream | 2026-10-23, the build-or-adopt audit |
| [F21](F21.md) | Try PP-OCRv6 in the PaddleOCR engine | feature | 2026-10-23, the build-or-adopt audit |
| [F23](F23.md) | hayro-syntax as a second reader in the lazy-reader corpus test | feature | 2026-10-23, the build-or-adopt audit |
| [F25](F25.md) | Upstream: hayro-cmap's codespace ranges, public | upstream | 2026-10-24, the content interpreter's wrap-up |
| [F26](F26.md) | skrifa past 0.46, for read-fonts' Type 1 parser of 0.43 and later | feature | 2026-10-24, the content interpreter's wrap-up |
| [F27](F27.md) | Research: what sheaf should read from a corrupt Flate stream (corpus 5208359) | research | 2026-10-24, the content interpreter's wrap-up |
| [F28](F28.md) | Register the `SHEF` prefix before the first release | feature | 2026-10-25, F24 |
| [F29](F29.md) | Research: how the Arlington tier holds a damaged form pdf-lint reads nothing of | research | 2026-10-26, the pdf-lint item's slice 1 |

Outside material already compared against sheaf: [sources
checked](../complete/backlog-sources-checked.md).
