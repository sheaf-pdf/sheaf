# M18 slice 11: pdf-objects' types through sheaf

Status: done · Part of [M18: the PDF layer](../../m18-pdf-layer/README.md) · in sheaf

**What:** the door's object types become pdf-objects', the migration's
real cost ([sheaf.md](../../m18-pdf-layer/sheaf.md#pdf-objects)). The conversion of
[slice 8](s08-sheaf-reads-through-pdf-structure.md) goes: pdf-structure
hands sheaf pdf-objects objects as they are.

- `sheaf_doc::pdf`'s `Object`, `Dictionary`, `Stream`, `ObjectId`,
  `StringFormat` and its constructor macro point at pdf-objects, and every
  call site moves to pdf-objects' API ([slice 3](s03-pdf-objects-editing-and-writing.md)).
- [Slice 10](s10-an-in-memory-document-of-sheaf-s-own.md)'s document
  holds pdf-objects objects; it moves into pdf-structure if the two agree.
- `serialize/` writes pdf-objects objects.
- The content lexer's operands and the interpreter's values, where they
  build objects, use pdf-objects'.

**Where:** sheaf-doc throughout; the 101 production files that named
lopdf before slice 1.

**Checked:**
- every test, unchanged; `materialize_corpus`: counts and output bytes
  as before; `source::lazy_tests`;
- the interop oracles and the Arlington tests;
- the 7k gate within its flags, with the conversion's cost gone;
- the door test: nothing outside `sheaf_doc::pdf` names pdf-objects' crate
  either, so the model stays swappable in one place.

## Notes

- Switched `sheaf_doc::pdf` directly onto `pdf_objects::{Object, Dict as Dictionary, Stream, ObjectId, Form as StringFormat, Value, Str, Name, Span, Data}` plus `StreamExt` (`pdf-filters` with `Backend::ZlibRs`) and `pdf::literal`, deleting `LazyPdf`'s object conversion layer from slice 8.
- Migrated all call sites across `sheaf-doc`, `sheaf-ops`, `sheaf-render`, `sheaf-cli`, `sheaf-app`, `sheaf-ocr`, `sheaf-plugin`, and `fuzz/` onto `pdf_objects`' API (`ObjectId { number, generation }`, `Option`-returning accessors, explicit `Object::name` / `pdf::literal`, and `Value` pattern matching).
- Corpus & 7k gate verification (with slices 12–13): `materialize_corpus` writes all `4,392 / 4,392` files (`2,246,374 / 2,272,748` surviving objects copied verbatim, 0 failed); `encrypted_corpus` keeps all `573 / 573` files in both Append and Rewrite; `rewrites_on_the_synthetic_and_heavy_7k` drops from slice 9's `166 ms / +53.6 MB` (synthetic) and `891 ms / +294 MB` (heavy) to `129 ms / +36.8 MB` (`0.97x` time, `0.60x` RSS vs M10.5 baseline) on synthetic 7k and `755 ms / +221 MB` (`1.07x` time, `0.88x` RSS, well inside the `1.2x` flag) on heavy 7k.
