# M18: the crate family

Part of [M18: the PDF layer](README.md): the crates, where pdf-lint's
modules go, where `LazyPdf` goes, and the rules across the family. What
sheaf takes from each crate is [sheaf.md](sheaf.md); what M18 does and
leaves is [scope.md](scope.md).

Sources: Greg's notes from another session, the doc "PDF Linter &
Sheaf: Crate Architecture Notes" (2026-10-26), which set out the layers
from pdf-lint's side; and pdf-lint's `0.2.0` branch at a104227, read
2026-10-27.

## Rulings

- **The family exposes writing** (encoding, encrypting, serializing)
  even where pdf-lint doesn't use it (Greg, 2026-10-27).
- **arlington takes pdf-cos's object types,** not the notes'
  `ObjectAccess` trait (Greg, 2026-10-27).
- **Probably its own milestone,** "even if it's mostly work in a
  separate tool/repo", "probably taking over lopdf"
  ([F13](../backlog/F13.md)) (Greg, 2026-10-27). M18 is proposed on
  that.
- **The crates are pdf-lint's agent's work:** "A lot of this work will
  happen over in pdf-lint under a different agent" (Greg, 2026-10-27).
  sheaf's agents still don't change pdf-lint's repositories; what
  sheaf's slices find there goes to Greg.
- **pdf-filters supports both Flate backends,** zlib-rs and
  miniz_oxide: "lint probably uses oxide, sheaf probably uses zlib for
  speed" (Greg, 2026-10-27). How: [slice 6](slices/s06-pdf-filters-and-pdf-crypt-for-writing.md).
- **Room for sheaf to be fast:** "we want to make sure that our
  abstractions leave room for sheaf to be fast and efficient" (Greg,
  2026-10-27). Decision 9 below.

## Why: bytes that say where they came from

A layer that keeps each value's bytes, and where they were, gives two
things lopdf's owned model can't:

- **Debugging.** Every value says where it was written. pdf-lint already
  reports findings that way (`[bytes 976..1053 of 31 0 R's data]`). A
  model that normalizes has lost a number's spelling (`1.50`, `+17`), a
  name's `#` escapes, and a key written twice.
- **Write-backs that copy what didn't change.** An object nothing
  touched is written as the bytes it was read from, not re-serialized.

## The family

The crates follow ISO 32000-2's clauses. Three exist, all MIT OR
Apache-2.0: pdf-tokenizer on crates.io, and pdf-filters and pdf-crypt in
pdf-lint's workspace, unpublished, which pdf-lint 0.2.0 reads through.

| Crate | ISO 32000-2 | State | What it is | In M18 |
|---|---|---|---|---|
| `pdf-tokenizer` | §7.2 | 0.1.0 | Lossless, zero-copy tokens; never rejects, never repairs; `no_std` | used |
| `pdf-cos` (or `pdf-syntax`) | §7.3 | to cut | One object at a time, lossless, with spans; owned and editable; serialized | yes |
| `pdf-filters` | §7.4 | 0.1.0, unpublished | The standard filters, every fault said, output capped; no image filters | encoding added |
| `pdf-crypt` | §7.6 | 0.1.0, unpublished | The standard security handler, decrypting | encrypting exposed |
| `pdf-structure` (or `doc`) | §7.5 | to cut | Cross-references, the trailer chain, incremental sections, object streams, recovery, a lazy resolver; writing files and updates | yes |
| `arlington` | | in pdf-lint | The model's TSVs and predicates, over pdf-cos's types | cut |
| content operators (`pdf-content`?) | §7.8 | considering | A content stream's operators and operands | later |
| `pdf-semantics` | §7.7 on | thinking | Typed views over resolved objects, saying where a value came from | later |
| `diag` | | thinking | Spans, issue codes and a sink through the layers | not needed |
| `pdf-repair` | | thinking | A corrected file, from fixes attached to lint findings | later |
| `pdf-lint` | | 0.1.0; 0.2.0 unreleased | Rules, profiles and reports; Apache-2.0 for the embedded model | on the crates |

The notes' writer is pdf-cos's serializer and pdf-structure's file
writer here, so each layer writes what it reads.

On the name: ISO's whole Clause 7 is titled "Syntax", so `pdf-syntax`
reads as all of the above. `pdf-cos` (COS, Adobe's and PDFBox's name for
the object system) says objects only, and leaves `pdf-structure` clearly
the file layer.

## Where pdf-lint's modules go

The library's modules depend on each other one way only, tests aside,
so most cuts move whole files. `bytes.rs` is the one to split.

```
pdf-tokenizer  §7.2  0.1.0
pdf-filters    §7.4  decodes       → tokenizer
pdf-crypt      §7.6  decrypts      → RustCrypto only
pdf-cos        §7.3  to cut        → tokenizer
pdf-structure  §7.5  to cut        → cos, filters, crypt
arlington            to cut        → cos
pdf-lint             what's left   → structure, arlington
```

| Crate | From pdf-lint | Where it stops |
|---|---|---|
| `pdf-cos` | `object.rs`, `text.rs`, `version.rs`, and `bytes.rs`'s object parser (`value`, `name`, an indirect object's header) | One object from tokens. It records both stream extents and resolves neither. |
| `pdf-structure` | the rest of `bytes.rs` (2,270 lines outside its tests), `filter.rs`, `source.rs` | The file: header, cross-reference sections along `/Prev`, recovery by scanning, object streams, budgets, the `Source` trait. Decrypting and decoding meet here. |
| `arlington` | `grammar.rs`, `predicate.rs`, `extension.rs`, `xxh64.rs`, `model/` | Tables and predicates, Apache-2.0. `predicate::Cx` resolves through a closure, so it needs only pdf-cos's types; `extension.rs` reads the catalog and XMP through `Source`, and is given them instead. |
| `pdf-lint` | `check.rs`, `allowance.rs`, `main.rs` | Rules, holds, allowances, reports, the command line. |

The test-only modules (`qpdf.rs`, `json.rs`, `write.rs`, `fuzz.rs`) go
with the tests that use them. The rules behind the cuts:

- **Codecs take values, not dictionaries.** pdf-crypt's `Dictionary` and
  pdf-filters' `Filter` are typed values, so neither depends on an
  object model. Reading them from a file is pdf-structure's: `filter.rs`
  (`/Filter` and `/DecodeParms`, as qpdf reads them) and the reader's
  `open` (the encryption dictionary).
- **What isn't encrypted is pdf-structure's.** pdf-crypt leaves it to
  the caller: the encryption dictionary's strings, a cross-reference
  stream, a signature's `/Contents`, metadata under `/EncryptMetadata
  false`. The notes put streams in structure for the same reason.
- **Direct dependencies.** 0.2.0's reader calls pdf-filters and
  pdf-crypt; qpdf comes in through `bytes::Decode`, `pub(crate)`, which
  only the tests give. So pdf-structure depends on both codecs, not on a
  trait.
- **Faults per crate.** Each codec has its `Fault`, its error keeping
  what decoded; the reader has `Warning`. A `diag` crate under the
  codecs would give the bottom layers a dependency they don't need.
- **pdf-cos with arlington.** The notes extract arlington first, but its
  predicates take `&Object`, so pdf-cos is cut in the same step.
- **`text::passwords` to pdf-crypt:** a typed password's byte forms for
  each revision, which any password prompt needs.

## Where LazyPdf goes: pdf-structure, not pdf-semantics

`LazyPdf` (`vendor/lopdf/src/lazy.rs`, 1,013 lines, sheaf's patch) is
the notes' lazy resolver, all of it §7.5:

- it reads the cross-reference sections and the trailer when it opens,
  each object when it's first asked for, and each object stream once;
- it decrypts as it reads (`encryption`), under caps (`ReadLimits`),
  over a `ByteSource` (in memory, or `FileBytes`, read by position);
- it says what the file is as written (`XrefFacts`, `base_sound`, the
  linearization dictionary's `/L`), and where each object is
  (`raw_span`, `raw_span_whole`).

Its one typed accessor is `catalog()`. So it's pdf-structure's other
strategy beside pdf-lint's eager reader: the notes' "one reader, two
strategies". [Slice 5](slices/s05-pdf-structure-lazy-reads.md) makes them one
reader, pdf-lint's read whole and sheaf's read on demand, with
`LazyPdf` as the model and the oracle, not code to copy: it's lopdf's
MIT code, and the crates stay free of its notice that way.

What's semantic is sheaf's layer over it, in `sheaf-doc/src/source/`:
the page table with its inherited keys, walked as PDFium walks it
(`pages.rs`), annotations' appearances (`appearance.rs`), XMP's
conformance claims (`conformance.rs`), and text strings
(`lopdf::decode_text_string`). Those are pdf-semantics candidates, but
they carry PDFium's rules and sheaf's needs, so they stay in sheaf for
M18 ([scope](scope.md#later)).

## Decisions across the family

1. **The owned type** in pdf-cos. Borrowed views are the easy half;
   sheaf edits. pdf-lint's `object.rs` is its start: each value owned,
   with its span, dictionaries as written.
2. **Where each object came from,** carried up from pdf-structure: a
   range in the file; a range in a container's decoded data (pdf-lint's
   `Span::stream` already); recovered; built by the caller. It makes a
   verbatim write-back safe, and lets a finding point at bytes.
3. **When a verbatim copy is valid,** in pdf-structure's writer, so
   every user gets the rules sheaf enforces by hand today:
   - **Encryption:** under pdf-crypt's `Method::Rc4` and `Aes128` a
     copy keeps its number and generation, and the file key; under
     `Aes256` (ISO 32000-2, Algorithm 1.A) the number may change.
   - **References:** a copy keeps the numbers it names, or has its
     references rewritten.
   - **`/Length` as a reference:** not copied, as `raw_span_whole`
     refuses today. **Recovered:** never copied.
   - An object-stream member's strings aren't encrypted one by one (the
     container is, whole), so its decoded bytes can go into a new
     container as they are.
4. **Read and write with a round trip:** parse, then write, gives the
   same bytes, at the cos and structure layers.
5. **Policy as a parameter:** ISO or PDFium for content, and whose
   recovery. M18 holds today's reading on the corpus; PDFium's recovery
   is [later](scope.md#later).
6. **Richness as a type parameter** (the notes): a sink sheaf passes as
   a no-op. Generic everywhere or `dyn` at some boundaries is measured
   on sheaf's 7k gate.
7. **Licensing.** Every crate but arlington and pdf-lint is MIT OR
   Apache-2.0, so sheaf ships them without changing its licence. Code
   moving out of pdf-lint (Apache-2.0) would take the family's terms,
   Greg's to relicense; sheaf's `serialize/` is MIT OR Apache-2.0
   already. Whatever holds the Arlington model stays Apache-2.0.
8. **Caps and fuzzing in every layer:** each takes limits, never panics,
   and has a fuzz body ([entry
   99](../research/build-or-adopt/99-fuzzing-property-tests.md)), as
   `source::fuzz` and pdf-lint's `fuzz.rs` do now.
9. **Room for sheaf to be fast** (Greg's ruling). What a checker wants
   (every fault said, every span kept) is never a cost sheaf has to
   pay:
   - **the caller picks what costs:** the Flate backend, a no-op sink
     (decision 6), lazy or whole reads, recording faults or not;
   - **no copy or allocation the caller didn't ask for:** spans rather
     than copies, bytes borrowed from a source in memory, decoding into
     a buffer the caller gives;
   - **generic on hot paths,** `dyn` only where a measurement says it
     costs nothing;
   - **shared across threads,** as `LazyPdf` is, so sheaf's parallel
     work (spans, containers) stays parallel;
   - **measured:** each crate slice reports its cost on sheaf's heavy 7k
     file beside what sheaf does today, and sheaf's slices hold the 7k
     gate.
