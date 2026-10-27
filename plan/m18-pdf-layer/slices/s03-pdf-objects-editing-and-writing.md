# M18 slice 3: pdf-objects, editing and writing

Status: open · Part of [M18: the PDF layer](../README.md) · in pdf-lint's workspace, by pdf-lint's agent

**What:** pdf-objects becomes the model sheaf edits and writes
([sheaf.md](../sheaf.md#pdf-objects)): the owned type of decision 1 in
[crates.md](../crates.md#decisions-across-the-family).

- **Editing:** build, change and remove values, covering
  [slice 1](s01-one-door-for-lopdf.md)'s inventory of what sheaf calls
  on lopdf's types. A value an edit changes loses its span, so the
  writer writes it anew; one no edit touched keeps it.
- **What it keeps:** reals at f64 with their spelling, dictionaries in
  order, a key written twice (every entry; a lookup takes the last), a
  string's form (literal or hex).
- **Writing:** `serialize/object.rs` moves in, byte for byte; it's
  sheaf's code, MIT OR Apache-2.0. sheaf keeps its own copy until
  [slice 12](s12-sheaf-writes-through-pdf-structure.md).
- **Caps** on depth and size, as pdf-lint's `NESTING` is now, from one
  limits value.

**From sheaf** ([from-sheaf.md](../from-sheaf.md#the-rest-of-the-writer-slices-3-6-and-7)):
`serialize/object.rs`, coupled only to lopdf's types.

**Where:** `crates/pdf-objects/`; from sheaf, `serialize/object.rs` and its
tests.

**Checked:**
- round trips: parse, then write, gives the same bytes for what sheaf
  writes; write, then parse, gives the same values for any value
  (property tests);
- `serialize/object.rs`'s tests, moved, unchanged;
- a fuzz body: parse and write any bytes without a panic, under the
  caps;
- pdf-lint's tests and corpus reports unchanged.
