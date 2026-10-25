# M10 proposal 21: Real forms come from Greg's corpus, outside the repo

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**Real forms come from Greg's corpus, outside the repo** (rewritten
at kickoff).
- `../pdf-corpus` is a separate repo of frozen real-world PDFs, kept
  out of sheaf so sheaf's history stays small. Its `forms/` folder has
  a row per file in `manifest.tsv` (kind, field counts by type, field
  scripts, license, sha256), and a SafeDocs pool of about 156,000
  form PDFs sits beside it for bulk searches.
- **Corpus tests look in `$SHEAF_CORPUS`, then `../pdf-corpus`, and
  skip with a message when neither exists** (the corpus README's
  convention). They aren't `#[ignore]`d: they run wherever the corpus
  is checked out. CI has no corpus (it has no public remote, and it
  holds restricted files), so CI runs the built-in fixtures only.
- **No PDF is committed to sheaf.** Tests that always run build their
  fixtures in code: hybrid forms with `/XFA` datasets, `/Perms /UR3`
  with a `/ByteRange`, `/SigFlags`, a structure tree with `/Tabs /S`;
  signed, permissions-only and XFA-only fixtures. The corpus tests
  check the same things on real forms.
- **Nothing is assumed about the corpus's contents.** It was still
  being built at kickoff. Calculation tests start on forms built in
  code and pdf.js's cases; once the pool is complete, a search for
  `/CO` and `AFSimple_Calculate` finds real ones for the checks.
- **Checks that name a planning form** (GA 500, OH IT 1040, VA 760,
  PA-40, NJ-1040, IL-1040, CO DR 0104, from the planning scratch
  corpus, which isn't kept) run on the corpus's equivalent when it
  has one, else on a fixture built in code with the same feature
  (Ohio's 319-field cascade becomes a 319-field synthetic one). The
  slice notes say which.
- Files marked `restricted` in the manifest (NY tax, California
  courts) are read in place; no copy, excerpt or rendering of them is
  committed.
- **Other renderers:** PDFium is lenient, so corpus and fixture tests
  also render outputs with poppler's `pdftoppm` (or `mutool draw`) and
  run `qpdf --check` on each, skipping with a message when the tool
  is missing. `pdftoppm` 26.01 is installed here; `qpdf` and `mutool`
  aren't.
- **Expected results for scripts:** JSON replay files (inputs, then
  the expected `/V` and formatted text) per scripted form, kept in
  sheaf and naming the corpus file by path and sha256; checked in
  slices 18 and 22.
