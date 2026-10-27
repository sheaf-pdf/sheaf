# M18 slice 11: pdf-cos's types through sheaf

Status: open · Part of [M18: the PDF layer](../README.md) · in sheaf

**What:** the door's object types become pdf-cos's, the migration's
real cost ([sheaf.md](../sheaf.md#pdf-cos)). The conversion of
[slice 8](s08-sheaf-reads-through-pdf-structure.md) goes: pdf-structure
hands sheaf pdf-cos objects as they are.

- `sheaf_doc::pdf`'s `Object`, `Dictionary`, `Stream`, `ObjectId`,
  `StringFormat` and its constructor macro point at pdf-cos, and every
  call site moves to pdf-cos's API ([slice 3](s03-pdf-cos-editing-and-writing.md)).
- [Slice 10](s10-an-in-memory-document-of-sheaf-s-own.md)'s document
  holds pdf-cos objects; it moves into pdf-structure if the two agree.
- `serialize/` writes pdf-cos objects, with sheaf's own
  `serialize/object.rs` still, so no output byte moves here.
- The content lexer's operands and the interpreter's values, where they
  build objects, use pdf-cos's.

**Where:** sheaf-doc throughout; the 101 production files that named
lopdf before slice 1.

**Checked:**
- every test, unchanged; `materialize_corpus`: counts and output bytes
  as before; `source::lazy_tests`;
- the interop oracles and the Arlington tests;
- the 7k gate within its flags, with the conversion's cost gone;
- the door test: nothing outside `sheaf_doc::pdf` names pdf-cos's crate
  either, so the model stays swappable in one place.
