# M18 slice 21: sheaf on pdf-semantics

Status: open · Part of [M18: the PDF layer](../README.md) · in sheaf

**What:** sheaf reads and writes the common structures through
pdf-semantics ([slice 20](s20-pdf-semantics-the-common-structures.md)),
and its own copies go.

- **Text strings:** `file.rs`'s `text_string`, and the three encoders
  in `save.rs`, `stamp_lib.rs` and `annot.rs`.
- **Dates:** `annot::pdf_date_now`. Showing a date
  (sheaf-ops' `fmt_pdf_date`) stays sheaf's.
- **Name and number trees:** `attachments.rs`, `labels.rs`, and the
  scripts' name tree read in `materialize/emit/form.rs`.
- **The page table** (`source/pages.rs`) on the page tree's PDFium
  walk; what sheaf keeps per page (`PageFacts`) stays sheaf's.
- **Conformance claims** (`source/conformance.rs`).
- `source/appearance.rs` stays sheaf's: it's what a flatten reads.

**Where:** `crates/sheaf-doc/src/`: those modules and their callers.

**Checked:**
- the page table's tests (`source/tests.rs`: pages numbered as PDFium
  numbers them), labels' and attachments' round trips, unchanged;
- `materialize_corpus`: counts and output bytes as before;
- the 7k gate's open, which walks the page tree, within its flag;
- every corpus suite of `just precommit`.
