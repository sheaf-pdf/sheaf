# M18 slice 11: pdf-objects' types through sheaf

Status: open · Part of [M18: the PDF layer](../README.md) · in sheaf

**What:** the door's object types become pdf-objects', the migration's
real cost ([sheaf.md](../sheaf.md#pdf-objects)). The conversion of
[slice 8](s08-sheaf-reads-through-pdf-structure.md) goes: pdf-structure
hands sheaf pdf-objects objects as they are.

- `sheaf_doc::pdf`'s `Object`, `Dictionary`, `Stream`, `ObjectId`,
  `StringFormat` and its constructor macro point at pdf-objects, and every
  call site moves to pdf-objects' API ([slice 3](s03-pdf-objects-editing-and-writing.md)).
- [Slice 10](s10-an-in-memory-document-of-sheaf-s-own.md)'s document
  holds pdf-objects objects; it moves into pdf-structure if the two agree.
- `serialize/` writes pdf-objects objects, with sheaf's own
  `serialize/object.rs` still, so no output byte moves here.
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
