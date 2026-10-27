# M18: scope

Part of [M18: the PDF layer](README.md): what M18 is for, how far it
goes, what was weighed for the family and left for later, and why.

## The goal: a coherent PDF crate family

Greg (2026-10-27): "dropping lopdf isn't exactly the goal here. a
coherent pdf crate family is." lopdf goes as part of it
([F13](../complete/backlog-closed.md#f13-in-house-what-sheaf-uses-of-lopdf-and-drop-the-dependency)), with the copies it blocked.

Coherent means, for this plan:

- **One crate per layer, both tools on it.** Each layer pdf-lint and
  sheaf both read is one crate, and both read through it: no layer
  implemented twice, once per tool.
- **Each layer writes what it reads** (Greg's ruling on writing), so a
  writer needs nothing outside the family.
- **One scanner, the policy a parameter** (the notes' "don't fork the
  parser"): ISO's rules for a validator, PDFium's for sheaf, which
  PDFium renders for.
- **One set of conventions:** caps on every reader, faults said where
  they are, no panics, a fuzz body per crate, MIT OR Apache-2.0 except
  what holds the Arlington model, and room for sheaf to be fast
  (decision 9 in [crates.md](crates.md#decisions-across-the-family)).

## The extent

M18 ends when pdf-lint and sheaf both read and write through the
family, and sheaf builds without lopdf:

| Crate | Slices | Both tools on it |
|---|---|---|
| pdf-tokenizer, with PDFium's rules as a mode | 18 | the scanner under pdf-objects and pdf-content-streams |
| pdf-objects | 2, 3, 11 | the object model, editing and writing |
| pdf-filters, pdf-crypt | 6, 12 | decoding and encoding, decrypting and encrypting |
| pdf-structure | 4, 5, 7, 8–10, 12–14 | reading lazily or whole, writing files and updates |
| pdf-content-streams | 16, 17, 19 | a page's operators, read and written |
| pdf-semantics, its common structures | 20, 21 | text strings, dates, name and number trees, the page tree, XMP claims |
| pdf-arlington | 2 | pdf-lint's checker, on pdf-objects |

Three gains for sheaf come with it, since they're why the layer keeps
spans at all ([sheaf.md](sheaf.md#what-sheaf-would-gain)): copies without
a second parse, object-stream members copied as decoded, and copies
under renumbering.

Nothing in it is for a user to see, beyond speed and size, so its proof
is that nothing else moved:

- `source::lazy_tests`: every object of the corpus's readable forms,
  read by the new reader, equals what sheaf read before;
- `materialize_corpus`: its counts as before, its copied count rising,
  and its outputs byte-identical wherever no slice meant to change them;
- `tier-lexer`: no page's tokens differ from PDFium's;
- the interop oracles (qpdf, pdf.js, PDFium) and the Arlington tests on
  what sheaf writes;
- the 7k gate within its flag, the fuzz bodies, and every corpus suite
  of `just precommit`;
- pdf-lint's own tests and corpus run, unchanged, after each cut.

## Two tracks

- **The crates** (slices 2–7, 16, 18 and 20; 19 starts in sheaf): in
  pdf-lint's workspace, by pdf-lint's own agent (Greg, 2026-10-27).
  They don't touch sheaf, and they go first: slice 2 starts M18 (Greg,
  2026-10-27). Their files here are sheaf's side of the spec; their rows follow
  what lands in pdf-lint.
- **sheaf** (slices 1, 8–15, 17 and 21; 14 and 19 add to the crates
  too): a milestone, never mixed into another (F13's "never
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
- **Writing after the model.** Slice 12 changes no output byte (D4
  aside), so it goes once the model is in, and slice 13 drops lopdf
  after it.
- **Move, then merge.** pdf-content-streams takes sheaf's lexer as it is (slice
  16) before its scanner moves onto pdf-tokenizer (slice 18), so each
  step has one thing to prove.
- **The gains where they're cheapest:** slice 9 right after the reader,
  slice 14 after the writer.

## Weighed for the family

What else could be in a coherent family, and where each stands
(2026-10-27). [from-sheaf.md](from-sheaf.md) has what each would start
from.

- **In M18:** pdf-content-streams (Greg: "definitely add these"), with its
  writing half (slice 19); PDFium's rules as a pdf-tokenizer mode (slice
  18), so the family has one scanner; pdf-semantics' common structures
  (slices 20–21), each of which both tools implement today.
- **The linearization crate** (between milestones, item 4): in the
  family, a crate of its own beside pdf-structure (Greg, 2026-10-27;
  [crates.md](crates.md#linearization-a-crate-of-its-own)), whose
  checker serves pdf-lint. It keeps its place between milestones, not
  an M18 slice: its plan needs no object model, and it assembles
  through sheaf's writer until pdf-structure's exists.
- **Later: fonts** (§9; `pdf-font`?). sheaf's simple-font encodings,
  the standard 14's metrics, Type 1 tokens and the predefined CMaps
  depend on nothing, but sheaf's font reading is tied to its
  interpreter and skrifa, and pdf-lint has no font checks. PDF/A's and
  PDF/UA's font rules (embedded programs, `ToUnicode`) are M14's and
  M17's ground: revisit there.
- **Later: color spaces and functions** (§8.6, §7.10). The only shared
  need today is how many components a named color space has, which
  pdf-content-streams' caller gives (`ResCx`).
- **Later: the rest of pdf-semantics:** the structure tree, the form's
  fields, actions and the like (pdf-lint's `Structure` and `Fields`;
  sheaf's forms and tagged content), extracted as both want the same
  view. The notes see it "largely generatable from Arlington"; the model
  is Apache-2.0, so check what generated code carries first.
- **Later: recovery as PDFium's.** sheaf's answer to the notes' "which
  reader's recovery": PDFium renders, so a file the model recovers one
  way and PDFium another shows two documents (`cpdf_parser.cpp` is the
  reference, entry 1). Slice 5 leaves the policy's place. M18 holds
  today's reading by count; a damaged file the new reader reads
  differently is named and held, not chased ("broken files aren't
  targets").
- **Later: pdf-repair.** The notes' clippy-style fixes, applied until the
  file lints clean: pdf-lint's `--fix`, and a sheaf "Repair document"
  command, which isn't in sheaf's plan. Its seam is kept: a finding can
  carry a fix, applied through pdf-structure's writer and pdf-content-streams'
  splice. sheaf's Rewrite already makes the notes' structural tier on
  every save; their "append-safe" fixes meet sheaf's Append, which keeps
  signatures; the conformance tier is M14's and M17's ground, and the
  notes' fit as WebAssembly plugins meets [document
  transforms](../topics/plugins/document-transforms.md). Test cases:
  [F29](../backlog/F29.md)'s two forms.
- **A convention, not a crate: `diag`.** Each crate's faults say where
  in its own input; pdf-objects' spans say where in the file; a sink is a
  type parameter sheaf passes as a no-op. A crate under the codecs would
  give the bottom layers a dependency they don't need.
- **Outside the family: image codecs** (DCT, JPX, CCITT, JBIG2).
  [Entry 33](../research/build-or-adopt/33-pdf-image-samples-image-s-own.md)
  owns them; PDFium decodes images for sheaf's pages.
- **Later: the other speed seams** ([sheaf.md](sheaf.md#the-notes-speed-seams-against-sheaf-today)):
  `memchr`, a budgeted cache of decoded streams, streaming decode. Each
  is measured against the 7k gate before it's taken.
- **Publishing** the crates: Greg's, once their APIs settle (the notes:
  one workspace, `publish = false`, until then).

## Not touched

- **[F15](../backlog/F15.md)** (a Rewrite that doesn't load the whole
  file) stays between milestones. If it lands first, the Rewrite graph
  leaves the eager `Document` before slice 12, which then has less to
  move.
- **Upstream lopdf fixes** ([entry
  2](../research/build-or-adopt/02-lopdf-fixes-upstream.md)) are
  goodwill once sheaf leaves lopdf: Greg's call, each PR confirmed
  first.
