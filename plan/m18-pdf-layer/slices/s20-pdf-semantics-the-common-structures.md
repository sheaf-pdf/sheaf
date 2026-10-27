# M18 slice 20: pdf-semantics: the common structures

Status: open · Part of [M18: the PDF layer](../README.md) · in pdf-lint's workspace, by pdf-lint's agent

**What:** pdf-semantics starts with the structures both tools implement
today, each once, read and written, over pdf-structure's resolver
(lazy or whole), each value saying where it came from (inherited,
defaulted), as the notes' views do.

- **Text strings** (ISO 32000-2, 7.9.2.2): PDFDocEncoding, UTF-16BE
  and UTF-8, decoded with every departure said (pdf-lint's `text.rs`),
  and encoded, which sheaf does in three places (`save.rs`,
  `stamp_lib.rs`, `annot.rs`).
- **Dates** (7.9.4): read and checked (pdf-lint's `is_date`) and
  written (sheaf's `annot::pdf_date_now`).
- **Name and number trees** (7.9.6, 7.9.7): walked under caps, a loop
  or a tree too deep said (pdf-lint's `Trees`; sheaf's
  `attachments::name_tree`, `labels`' number trees and the scripts'
  name tree in `materialize/emit/form.rs`), and written (sheaf's
  `labels::number_tree`).
- **The page tree** (7.7.3) with its inherited attributes, walked by a
  policy: PDFium's (sheaf's `source/pages.rs`: depth first, a kid with
  `/Kids` a node whatever its `/Type`, stopping at `/Count` where PDFium
  does) or ISO's (pdf-lint's walk).
- **XMP claims** (14.3.2): PDF/A and PDF/UA conformance (sheaf's
  `source/conformance.rs`) and the extension subsets (pdf-lint's
  `extension.rs`), one XMP reader for both.

Not generated from Arlington here: the rest of pdf-semantics, and
whether generated code carries the model's terms, are
[later](../scope.md#weighed-for-the-family).

**From sheaf** ([from-sheaf.md](../from-sheaf.md#pdf-semantics-slices-20-and-21)).

**Where:** `crates/pdf-semantics/`.

**Checked:**
- both tools' tests of these, moved, unchanged;
- pdf-lint's reports on sheaf's corpus, unchanged;
- round trips: written, then read, gives what was written;
- a fuzz body for the walks: loops, depth, any bytes, no panic.
