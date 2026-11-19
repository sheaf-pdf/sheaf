# M18 slice 9: Copies without a second parse, and object-stream members copied

Status: done · Part of [M18: the PDF layer](../../m18-pdf-layer/README.md) · in sheaf

**What:** the first two gains of [sheaf.md](../../m18-pdf-layer/sheaf.md#what-sheaf-would-gain).

- **No second parse.** `Body::Raw` takes each object's span as the
  first parse recorded it. `raw_span_whole`'s second parse, and the
  threads it runs on (`materialize/mod.rs:572-599`), go.
- **Object-stream members copied.** A member nothing changed goes into
  its new container as its decoded bytes, where the copy rules allow
  (decision 3 in [crates.md](../../m18-pdf-layer/crates.md#decisions-across-the-family)):
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

## Notes

Done 2026-11-18:

- `LazyPdf::to_document_and_raw` collects `RawCopies` (outer-file
  `Origin::File` whole spans and decoded `Origin::Stream` member byte
  slices from `Document::held`, excluding containers with
  `in_the_clear == true`) during the single `Reading::to_document` pass.
  `Spans` and `raw_spans`'s second-parse thread pool in
  `crates/sheaf-doc/src/materialize/mod.rs` are deleted.
- `Body::Member` in `crates/sheaf-doc/src/serialize/full.rs` writes
  unmodified packable object-stream members directly into their new
  containers without re-serializing, falling back to `Body::Object` when
  unpacked or past `MAX_DATA`.
- `materialize_corpus` verbatim copies rose from 921,586 (slice 8;
  919,465 before M18) to **2,246,369 of 2,267,675** surviving primary
  objects (99.1%), with 0 failures across all 4,388 corpus forms.
- `gate_7k` (`rewrites_on_the_synthetic_and_heavy_7k`, `--release`):
  synthetic 7k Rewrite 166 ms / +53.6 MB RSS, heavy 7k Rewrite 891 ms /
  +294 MB RSS (while `to_document_and_raw` still bridges `pdf_objects` to
  `lopdf::Object` ahead of slice 11).
- Aligned sparse object numbers past `NUMBERS` (8,388,607) with
  `pdf_structure::Limits::entries` (entry-count cap rather than max object
  number): `BitSet` stores sparse `>= NUMBERS` numbers in a `BTreeSet`
  without dense word allocation, `number::stable` ignores dangling
  `>= NUMBERS` references absent from `loaded`, and `materialize::tests::far`
  places its two high object-stream members via sparse type-2 `/Index`
  entries.
