# M18 slice 8: sheaf reads through pdf-structure

Status: open · Part of [M18: the PDF layer](../README.md) · in sheaf

**What:** `source/` reads through pdf-structure's lazy reader in place
of `LazyPdf`. The model is still lopdf's: objects are converted to
lopdf's types at the door ([slice 1](s01-one-door-for-lopdf.md)), a
conversion that lives only until [slice 11](s11-pdf-objects-types-through-sheaf.md).
Reading is the riskiest change, so it goes alone, where the two readers
can be compared object by object.

- `Source::open` opens with pdf-structure, under `Limits` mapped to its
  limits value, and the password given.
- `Source::object`, `raw_span`, `raw_span_whole`, `xref`, `base_sound`
  and the linearization check (`holds_linearization`) come from it.
- Its errors map to what sheaf says today: a password needed, an object
  missing, a stream over its cap.
- The eager `Document` still loads through lopdf until
  [slice 10](s10-an-in-memory-document-of-sheaf-s-own.md).

**Where:** `crates/sheaf-doc/src/source/`, `sheaf_doc::pdf`.

**Checked:**
- `source::lazy_tests`, now pdf-structure's lazy read against lopdf's
  eager load: every object of the corpus's readable forms equal;
- `materialize_corpus`: counts and output bytes as before;
- every corpus suite of `just precommit`, `source::fuzz`, the interop
  oracles;
- the 7k gate's open and Rewrite within their flags;
- a file the two readers read differently is named and held by exact
  count, never skipped; a damaged one isn't chased ("broken files
  aren't targets"). Where PDFium agrees with lopdf and not the new
  reader, that's a bug in this slice.
