# M18 slice 14: Copies under renumbering

Status: open · Part of [M18: the PDF layer](../README.md) · in sheaf, and pdf-structure's writer

**What:** the third gain of [sheaf.md](../sheaf.md#what-sheaf-would-gain):
an unchanged object is copied even when its numbers change, its
references rewritten in place.

- Today a profile that renumbers writes everything anew, since a copy
  names others by number (`Numbering::Stable` in the raw rule).
- With spans on every value, the writer copies an object's bytes and
  rewrites only its references' spans, under decision 3's rules
  ([crates.md](../crates.md#decisions-across-the-family)): under RC4
  and AES-128 the object's own number keys its strings, so an encrypted
  object copies only under AES-256 or when its number stays.
- The rewriting belongs in pdf-structure's writer, so every user gets
  it.

The milestone's end doesn't wait for this slice: lopdf is gone after
[slice 13](s13-lopdf-gone.md). It's planned because it's one of the
reasons the layer keeps spans.

**Where:** pdf-structure's writer; `materialize/mod.rs`'s raw rule.

**Checked:**
- `materialize_corpus`'s copied count on the renumbering profiles,
  before and after;
- every renumbered file read back equal to its re-serialized form;
- the interop oracles and the Arlington tests;
- a test per copy rule that fails without it.
