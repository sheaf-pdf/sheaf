# M18: scope

Part of [M18: the PDF layer](README.md): how far M18 goes, what waits
for later, and why. Greg (2026-10-27): "We should plan out how to do
this (and what extent to do). I think this is probably taking over
lopdf".

## The extent: lopdf gone, and the copies it blocked

M18 ends when sheaf builds without lopdf
([F13](../backlog/F13.md)), reading and writing through pdf-tokenizer,
pdf-cos, pdf-filters, pdf-crypt and pdf-structure, which pdf-lint reads
through too. Three gains come with it, since they're why the layer keeps
spans at all ([sheaf.md](sheaf.md#what-sheaf-would-gain)): copies without
a second parse, object-stream members copied as decoded, and copies
under renumbering.

That's the smallest extent that drops lopdf: every crate in it replaces
something of lopdf's that sheaf uses. Nothing is in it for a
user to see, beyond speed and size, so its proof is that nothing else
moved:

- `source::lazy_tests`: every object of the corpus's readable forms,
  read by the new reader, equals what sheaf read before;
- `materialize_corpus`: its counts as before, its copied count rising,
  and its outputs byte-identical wherever no slice meant to change them;
- the interop oracles (qpdf, pdf.js, PDFium) and the Arlington tests on
  what sheaf writes;
- the 7k gate within its flag, the fuzz bodies, and every corpus suite
  of `just precommit`;
- pdf-lint's own tests and corpus run, unchanged, after each cut.

## Two tracks

- **The crates** (slices 2–7): in pdf-lint's workspace, by pdf-lint's
  own agent (Greg, 2026-10-27). They don't touch sheaf, so they could go
  ahead of sheaf's milestone order. Their files here are sheaf's side of
  the spec; their rows follow what lands in pdf-lint.
- **sheaf** (slices 1 and 8–15; slice 14 adds to pdf-structure's
  writer too): a milestone, never mixed into another (F13's "never
  mid-milestone"). Slice 1 needs no crate and can go first.

## The order, and why

- **One door first.** Slice 1 sends every sheaf crate to lopdf through
  `sheaf_doc::pdf`, so the later swaps change one module's target and
  then the calls, and its inventory tells slices 3 and 10 what the owned
  types must offer.
- **Read before the model.** The reader is the riskiest change and the
  easiest to prove alone: slice 8 reads through pdf-structure and hands
  sheaf lopdf's types, converted at the door, so `lazy_tests` compares
  the two readers object by object. The conversion lives only inside
  M18, until slice 11.
- **The eager `Document` before the types.** Slice 10 gives sheaf an
  in-memory document of its own over the door's types, so slice 11's
  type swap carries it along instead of converting at a second
  boundary.
- **Writing last.** Slice 12 changes no output byte (D4 aside), so it
  goes once the model is in, and slice 13 drops lopdf after it.
- **The gains where they're cheapest:** slice 9 right after the reader,
  slice 14 after the writer.

## Later

Each waits for a user that needs it; none is needed to drop lopdf.

- **The content operators crate.** `content/` is sheaf's own already
  (lexer, writer, splice), and `lopdf::content` is only a test oracle.
  Its PDFium rules would become a policy, beside ISO's for a validator,
  when M13's content editing or a pdf-lint content check wants it.
  What it would start from: [from-sheaf.md](from-sheaf.md#content-the-content-crate-later).
- **pdf-semantics.** sheaf's candidates are `source/pages.rs` (the page
  table, inherited keys, walked as PDFium walks it), `appearance.rs`,
  `conformance.rs`, and text strings; pdf-lint's are `text.rs` and its
  predicates' walks of name trees, the structure tree and field names
  (`Trees`, `Structure`, `Fields`). Extract a view when both want the
  same one. The notes see it "largely generatable from Arlington"; the
  model is Apache-2.0, so check what generated code carries first.
- **Recovery as PDFium's.** sheaf's answer to the notes' "which reader's
  recovery": PDFium renders, so a file the model recovers one way and
  PDFium another shows two documents (`cpdf_parser.cpp` is the
  reference, entry 1). M18 holds today's reading by count; a damaged
  file the new reader reads differently is named and held, not chased
  ("broken files aren't targets").
- **pdf-repair.** The notes' clippy-style fixes, applied until the file
  lints clean: pdf-lint's `--fix`, and a sheaf "Repair document"
  command, which isn't in sheaf's plan. sheaf's Rewrite already makes
  the notes' structural tier on every save; the notes' "append-safe"
  fixes meet sheaf's Append, which keeps signatures; the conformance
  tier is M14's and M17's ground, and the notes' fit as WebAssembly
  plugins meets [document transforms](../topics/plugins/document-transforms.md).
  Test cases: [F29](../backlog/F29.md)'s two forms.
- **A `diag` crate.** Each crate's own faults suffice ([crates.md](crates.md#where-pdf-lints-modules-go)).
  sheaf would use the recoveries for Properties or a "this file was
  damaged" note, where lopdf's recovery is silent today.
- **The other speed seams** ([sheaf.md](sheaf.md#the-notes-speed-seams-against-sheaf-today)):
  `memchr`, a budgeted cache of decoded streams, streaming decode. Each
  is measured against the 7k gate before it's taken.
- **Publishing** the crates: Greg's, once their APIs settle (the notes:
  one workspace, `publish = false`, until then).

## Not touched

- **The linearization crate** (between milestones, item 4) is
  independent of any object model.
- **[F15](../backlog/F15.md)** (a Rewrite that doesn't load the whole
  file) stays between milestones. If it lands first, the Rewrite graph
  leaves the eager `Document` before slice 12, which then has less to
  move.
- **Upstream lopdf fixes** ([entry
  2](../research/build-or-adopt/02-lopdf-fixes-upstream.md)) are
  goodwill once sheaf leaves lopdf: Greg's call, each PR confirmed
  first.
