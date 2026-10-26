# Build or adopt 66: Object-stream packing

Area: M14: Document processing · Part of [build or adopt](README.md)

- **Need:** pack eligible objects into object streams on Rewrite (and
  Append), with ISO 32000-1 7.5.7's exclusions, encrypting each
  container once.
- **Where:** `ROADMAP.md:436-437`; M10.5 §20
  (`m10.5-edit-api.md:3659-3664`); decisions (`m10.5-decisions.md:530`,
  `:1957`).
- **Today's plan:** "Packing in Rewrite and Append, with
  `packable(ObjKey, &SymObject, gen)` encoding ISO 32000-1 7.5.7's
  exclusions … Member strings are never encrypted individually; the
  container is" (`m10.5-edit-api.md:3659-3664`).
- **Candidates:** lopdf 0.46's `ObjectStreamBuilder` and `SaveOptions`
  (`use_object_streams`), about 350 lines, only for lopdf's own
  `Document` writer; pdf-writer has no object-stream builder that was
  found; qpdf (`--object-streams=generate`) is C++.
- **Fit:** the packing is easy (a header of number–offset pairs, the
  objects, Flate). The hard parts are sheaf's: the exclusions,
  encryption, which members a signature needs left in place, the
  xref-stream base. Those already live in the serializer.
- **Recommendation:** **build in-house** in `serialize/`, with lopdf's
  `object_stream.rs` as a reference. Check with `qpdf --check`, PDFium
  and pdf.js reading the result back, and the corpus. High.
- **Built:** in-house, between milestones
  ([object streams](../../complete/object-streams.md), slice 2,
  2026-10-26): `serialize/objstm.rs` (`packable`, the container),
  `group.rs` (by the pages that use each object), type-2 entries and a
  compressed cross-reference stream in `xref.rs`, packing in `full.rs`.
  On a Rewrite and every export of a file that used object streams; not
  on an Append ([B62](../../backlog/B62.md)).
