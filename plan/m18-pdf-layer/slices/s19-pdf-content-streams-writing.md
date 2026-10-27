# M18 slice 19: pdf-content-streams, writing

Status: open · Part of [M18: the PDF layer](../README.md) · in sheaf, then pdf-lint's workspace

**What:** pdf-content-streams writes what it reads (Greg's ruling on writing):
sheaf's content writer and splice move into it.

- **The writer** (`content/writer.rs`): operators with their operands
  written the same way every time (at most four decimals, no exponent,
  no `-0`), resources named by digest when the content is finished.
- **The splice** (`content/splice.rs`): ranges taken out of a page's
  streams, every other byte as it was, only where that can't change what's
  drawn after. pdf-repair would take a broken operator out with it.
- **First, in sheaf:** the writer reaches sheaf's fonts (`Font`,
  `Unencoded`), `annot`'s and `geom`'s geometry, and `page.rs`'s
  `ReadObjects`. Text goes in as codes a font already encoded, with
  that font's resource, so sheaf's fonts stay sheaf's
  ([scope](../scope.md#weighed-for-the-family): fonts are later); the
  geometry comes as plain numbers; resources by digest (`res.rs`, on
  lopdf's types alone) move with it.
- Then the two move, and sheaf writes and splices through pdf-content-streams.

**From sheaf** ([from-sheaf.md](../from-sheaf.md#content-pdf-content-streams-slices-16-to-19)):
`content/writer.rs`, `content/res.rs`, `content/splice.rs`, and their tests
(`writer_tests.rs`, the splice cases of `blocks_tests.rs`).

**Where:** `crates/sheaf-doc/src/content/`, then `crates/pdf-content-streams/`.

**Checked:**
- the writer's and splice's tests, moved, unchanged;
- every byte sheaf writes, as before: `materialize_corpus`'s outputs,
  the release properties, the seeds;
- what's written reads back through pdf-content-streams' iterator as written.
