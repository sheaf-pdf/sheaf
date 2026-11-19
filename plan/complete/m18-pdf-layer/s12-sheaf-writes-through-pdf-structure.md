# M18 slice 12: sheaf writes through pdf-structure

Status: done · Part of [M18: the PDF layer](../../m18-pdf-layer/README.md) · in sheaf

**What:** sheaf's `serialize/` keeps the policy and hands the writing to
the crates ([slice 7](s07-pdf-structure-writing.md)).

- Objects through pdf-objects' serializer; sheaf's `serialize/object.rs`
  goes.
- Cross-reference sections, object-stream containers, whole files and
  incremental sections through pdf-structure (`serialize/xref.rs` and
  `serialize/objstm.rs` go).
- Compression through pdf-filters, zlib-rs named at every call (D4);
  encryption through pdf-crypt and `pdf_structure::write::Encryptor`,
  drawing fresh 16-byte OS CSPRNG AES IVs in production per ISO 32000-2
  §7.6.3.2 & §7.6.3.3 (`pdf_crypt::Ivs::new()`), with `insecure-test-writer`
  gated strictly to dev/test fixtures.
- sheaf keeps: what to write and its numbers (`ObjKey`, the profiles,
  `keep_only`), grouping by page (until the linearization crate takes
  it), durable writes.

**Where:** `crates/sheaf-doc/src/serialize/`, `materialize/`.

**Checked:**
- unencrypted and RC4 saved files deterministic across runs and threads;
  AES saves draw fresh OS random IVs and round-trip identically;
  [B26](../backlog-closed.md#b26-the-flate-backend-depends-on-which-crates-are-in-the-build)
  closed (zlib-rs everywhere);
- encrypted saves: every revision and method round trips, file keys wiped
  on drop ([F19](../backlog-closed.md#f19-wipe-file-keys-not-only-passwords)),
  and `Crypt::packs`' Identity streams under an encrypting `/StrF` hold;
- Append keeps signatures: the signature tests, unchanged;
- the interop oracles and the Arlington tests.

## Notes

- Deleted `crates/sheaf-doc/src/serialize/{object,objstm,xref}.rs`, routing object syntax through `pdf_objects::serialize`, cross-reference tables/streams and object-stream containers through `pdf_structure::write` (`Backend::ZlibRs`), and stream compression/decompression through `pdf-filters` (`Backend::ZlibRs`), closing B26.
- Rewrote `serialize::crypt::Crypt` over `pdf_structure::write::Encryptor` (`pdf-crypt` with `Zeroizing` keys, closing F19). Production saves draw fresh 16-byte OS CSPRNG AES IVs (`pdf_crypt::Ivs::new()`); deterministic `insecure-test-writer` (`pdf_crypt::writer` and `Ivs::derived`) is enabled only in `[dev-dependencies]` and the `fixtures` feature, enforced by `just boundaries` and CI.
