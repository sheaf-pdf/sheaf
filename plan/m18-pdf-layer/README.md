# M18: The PDF layer

Status: proposed · planned 2026-10-27; its place in the order waits on Greg

Proves: sheaf reads and writes PDF through its own crates, with lopdf gone, and copies more of what didn't change · Depends on: nothing unbuilt; [F15](../backlog/F15.md) first (soft)

## Summary

sheaf takes over lopdf ([F13](../backlog/F13.md)) through a family of
crates split from [pdf-lint](https://github.com/sheaf-pdf/pdf-lint),
which Greg owns, each keeping the bytes it read and where they were
(Greg, 2026-10-27: "This is probably it's own milestone on its own (even
if it's mostly work in a separate tool/repo)... I think this is probably
taking over lopdf").

- **The crates** ([crates.md](crates.md)): pdf-tokenizer (§7.2, on
  crates.io), pdf-cos (§7.3, the object model, cut from pdf-lint),
  pdf-filters (§7.4) and pdf-crypt (§7.6), which exist and gain
  writing, and pdf-structure (§7.5, cross-references, object streams,
  the lazy reader and the file writer). arlington is cut too, on
  pdf-cos's types. pdf-lint reads through the same crates.
- **`LazyPdf` is pdf-structure's,** its lazy strategy beside pdf-lint's
  read whole; it isn't pdf-semantics, which would be the typed layer
  sheaf builds over it in `source/` ([crates.md](crates.md#where-lazypdf-goes-pdf-structure-not-pdf-semantics)).
- **In sheaf** ([sheaf.md](sheaf.md)): one door to the PDF types, the
  reader, an in-memory document of sheaf's own, the model, the writer,
  then lopdf deleted.
- **The gains** that keeping spans buys: copies without a second parse,
  object-stream members copied as decoded, and copies under
  renumbering.
- **Later, not M18** ([scope](scope.md#later)): the content operators
  crate, pdf-semantics, PDFium's recovery as a policy, pdf-repair, a
  `diag` crate, the other speed seams, publishing.

**Rulings** (Greg, 2026-10-27; [crates.md](crates.md#rulings)):
- the family exposes writing even where pdf-lint doesn't use it;
- arlington takes pdf-cos's types;
- the crate slices are done in pdf-lint's workspace by pdf-lint's own
  agent ("A lot of this work will happen over in pdf-lint under a
  different agent");
- pdf-filters supports both Flate backends, zlib-rs and miniz_oxide:
  pdf-lint probably on miniz_oxide, sheaf probably on zlib-rs, for
  speed;
- the abstractions leave room for sheaf to be fast and efficient.

## Where it stands

Status: **proposed** (2026-10-27). Planned from the memo of 2026-10-26
(now [sheaf.md](sheaf.md)), Greg's "PDF Linter & Sheaf: Crate
Architecture Notes" and pdf-lint's `0.2.0` branch (a104227), in which
pdf-crypt is built and pdf-lint reads without qpdf. Nothing is started,
and more planning is to come (Greg). Waiting on Greg
([asks](../asks.md)): its place in the order, and the open questions
below.

It has two tracks ([scope](scope.md#two-tracks)): slices 2–7 in
pdf-lint's workspace, by pdf-lint's agent, which touch nothing of
sheaf's (their files here are sheaf's side of the spec, and their rows
follow what lands there), and slices 1 and 8–15 in sheaf, a milestone
of their own. Each sheaf slice is proved by
what didn't move: `source::lazy_tests`, `materialize_corpus`'s counts
and bytes, the interop oracles, the Arlington tests and the 7k gate.

## Slices

| Slice | Title | Status | Depends on | Where |
|---|---|---|---|---|
| [1](slices/s01-one-door-for-lopdf.md) | One door for lopdf | open | — | sheaf |
| [2](slices/s02-pdf-cos-and-arlington-cut-from-pdf-lint.md) | pdf-cos and arlington, cut from pdf-lint | open | — | pdf-lint |
| [3](slices/s03-pdf-cos-editing-and-writing.md) | pdf-cos, editing and writing | open | 1, 2 | pdf-lint |
| [4](slices/s04-pdf-structure-cut-from-pdf-lint.md) | pdf-structure, cut from pdf-lint | open | 2 | pdf-lint |
| [5](slices/s05-pdf-structure-lazy-reads.md) | pdf-structure, lazy reads | open | 4 | pdf-lint |
| [6](slices/s06-pdf-filters-and-pdf-crypt-for-writing.md) | pdf-filters and pdf-crypt, for writing | open | — | pdf-lint |
| [7](slices/s07-pdf-structure-writing.md) | pdf-structure, writing | open | 3, 5, 6 | pdf-lint |
| [8](slices/s08-sheaf-reads-through-pdf-structure.md) | sheaf reads through pdf-structure | open | 1, 5 | sheaf |
| [9](slices/s09-copies-without-a-second-parse.md) | Copies without a second parse, and object-stream members copied | open | 8 | sheaf |
| [10](slices/s10-an-in-memory-document-of-sheaf-s-own.md) | An in-memory document of sheaf's own | open | 1, 8; F15 (soft) | sheaf |
| [11](slices/s11-pdf-cos-s-types-through-sheaf.md) | pdf-cos's types through sheaf | open | 3, 10 | sheaf |
| [12](slices/s12-sheaf-writes-through-pdf-structure.md) | sheaf writes through pdf-structure | open | 7, 11; D4 (ruling) | sheaf |
| [13](slices/s13-lopdf-gone.md) | lopdf gone | open | 12 | sheaf |
| [14](slices/s14-copies-under-renumbering.md) | Copies under renumbering | open | 9, 12 | sheaf, pdf-lint |
| [15](slices/s15-wrap-up.md) | Wrap-up | open | 1–14 | sheaf |

## Open questions

In [asks](../asks.md), one line:

- **Its place:** rec. the crate slices whenever Greg wants them, since
  they don't touch sheaf; sheaf's slices as a milestone after the work
  between milestones, before M11, whose passwords would otherwise be
  built on lopdf's handler and then moved.
- **Names:** `pdf-cos` or `pdf-syntax`, `pdf-structure` or `doc`.
- **The hash:** the notes suggest BLAKE3 for both tools. sheaf's
  xxh3-128 names blobs within sheaf, and `store.rs` refuses a collision;
  BLAKE3 matters where a hash leaves sheaf (a report, a shared cache).
  Benchmark first, as the notes say. Not needed for M18.
- **D4,** which Flate backend every sheaf build uses, which slice 12
  writes with. Greg leans zlib-rs, for speed (2026-10-27); pdf-filters
  supports both either way (slice 6).

## Plan files

- [The crates](crates.md): the family, where pdf-lint's modules go,
  where `LazyPdf` goes, the decisions across the family
- [What sheaf takes](sheaf.md): where sheaf stands, its use of lopdf,
  crate by crate, the gains
- [Scope](scope.md): the extent, the two tracks, the order, what's later
- Build or adopt: [entry 1](../research/build-or-adopt/01-pdf-object-layer.md)
  (the object layer), [entry 2](../research/build-or-adopt/02-lopdf-fixes-upstream.md)
  (lopdf fixes upstream), [entry 29](../research/build-or-adopt/29-standard-security-handler-read-r2-r6.md)
  (the security handler)
