# M18 slice 12: sheaf writes through pdf-structure

Status: open · Part of [M18: the PDF layer](../README.md) · in sheaf

**What:** sheaf's `serialize/` keeps the policy and hands the writing to
the crates ([slice 7](s07-pdf-structure-writing.md)), byte for byte.

- Objects through pdf-objects' serializer; sheaf's `serialize/object.rs`
  goes.
- Cross-reference sections, object-stream containers, whole files and
  incremental sections through pdf-structure.
- Compression through pdf-filters, zlib-rs named at every call (D4);
  encryption through
  pdf-crypt, with sheaf's keyed-hash IVs (M10.5 §11.7), `same_crypt`
  answered by its `Method`.
- sheaf keeps: what to write and its numbers (`ObjKey`, the profiles,
  `keep_only`), grouping by page (until the linearization crate takes
  it), durable writes.

**Where:** `crates/sheaf-doc/src/serialize/`, `materialize/`.

**Checked:**
- every saved file byte for byte as before: `materialize_corpus`'s
  outputs, the determinism and release-property tests, the seeds;
  if [B26](../../backlog/B26.md)'s fix hasn't landed first, the switch
  of sheaf-cli's and the tests' builds to zlib-rs alone, in its own
  commit;
- encrypted saves: every revision and method round trips, and
  `Crypt::packs`' Identity streams under an encrypting `/StrF` hold;
- Append keeps signatures: the signature tests, unchanged;
- the interop oracles and the Arlington tests.
