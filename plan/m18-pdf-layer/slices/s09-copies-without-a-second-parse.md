# M18 slice 9: Copies without a second parse, and object-stream members copied

Status: open · Part of [M18: the PDF layer](../README.md) · in sheaf

**What:** the first two gains of [sheaf.md](../sheaf.md#what-sheaf-would-gain).

- **No second parse.** `Body::Raw` takes each object's span as the
  first parse recorded it. `raw_span_whole`'s second parse, and the
  threads it runs on (`materialize/mod.rs:572-599`), go.
- **Object-stream members copied.** A member nothing changed goes into
  its new container as its decoded bytes, where the copy rules allow
  (decision 3 in [crates.md](../crates.md#decisions-across-the-family)):
  same encryption, references kept. Members are re-serialized today
  because they have no span.

**Where:** `materialize/mod.rs` (the raw rule, `Spans`),
`serialize/objstm.rs`, `serialize/full.rs`.

**Checked:**
- the spans equal `raw_span_whole`'s on every corpus object before it
  goes, so the first change moves no output byte;
- `materialize_corpus`'s copied count rises (919,465 of 2,269,688
  before), its other counts unchanged;
- what changes (members copied): the interop oracles, the Arlington
  tests, and every saved file read back equal;
- the 7k gate's Rewrite (808 ms at #57), measured before and after.
