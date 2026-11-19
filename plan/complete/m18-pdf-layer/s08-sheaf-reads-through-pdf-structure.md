# M18 slice 8: sheaf reads through pdf-structure

Status: done · Part of [M18: the PDF layer](../../m18-pdf-layer/README.md) · in sheaf

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

## Notes

Done 2026-11-18:

- `crates/sheaf-doc/src/pdf/reader.rs` replaces lopdf's `LazyPdf` with
  `pdf_structure::Reading` (`Backend::ZlibRs`, `Recovery::Verified`),
  converting objects to `lopdf::Object` at the `sheaf_doc::pdf` door until
  slice 11, and exposing `LazyPdf::to_document` via
  `Reading::to_document` so `materialize::graph::eager` reads from the
  same reader.
- `source::lazy_tests` compares all 4,389 readable corpus forms
  (2,989,321 objects × 2 = 5,978,642 lazy reads from memory and
  `FileBytes`): 82 files differ from lopdf's eager load, every one held
  by exact count in `EXPLAINED` where `pdf-structure` agrees with qpdf
  and PDFium (multi-EOL whitespace before `endstream`, component `/Type
  /Metadata` streams under `/EncryptMetadata false`, incremental `type 0`
  free entries, `1263925.pdf`'s unpadded `/DigestValue`, and `0630899.pdf`'s
  recovered object `1379 0`).
- `materialize_corpus` baseline moved from `(4389, 2_269_688, 919_465,
  432)` to `(4388, 2_267_675, 921_586, 431)`: `4422502.pdf`'s damaged
  trailer lacking `/Root` is refused alike by sheaf and `pdf-lint`
  (closing [F29](../backlog-closed.md#f29-research-how-the-arlington-tier-holds-a-damaged-form-pdf-lint-reads-nothing-of)
  and dropping its `arlington_allowed.tsv` row), freed `type 0` objects
  go, and 2,121 outer-file streams with multi-EOL whitespace before
  `endstream` now copy verbatim.
- Logged [F30](../../backlog/F30.md) for upstream `pdf-structure` XRef
  stream `/Size`/`/Index` cap check before inflate and hybrid `/XRefStm`
  deduplication across `/Prev` sections.
