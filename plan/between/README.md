# Between milestones

Status: in progress

Ordered by reach, not milestone (Greg, 2026-10-23). These come first,
in this order:

1. ~~**The content interpreter:** M11's slice, pulled forward~~: done
   (#50).
2. ~~**Object streams, and B3**~~: done
   ([object-streams.md](../complete/object-streams.md)). A Rewrite and
   every export of a file that used object streams pack them again,
   grouped by the pages that use them, each container encrypted once:
   the IRS forms now shrink by a median 12.9% on a Save As (they grew by
   half), and Properties' Fast Web View holds after an incremental save
   (B3). Packing on Append is [B62](../backlog/B62.md).
3. **sheaf's Arlington checks on pdf-lint:** in progress,
   [pdf-lint.md](pdf-lint.md) (Greg, 2026-10-26). sheaf's checker was
   extracted and published as
   [pdf-lint](https://github.com/sheaf-pdf/pdf-lint); the tests move to
   it as a dev-dependency, CI installs it with `setup-pdf-lint`, and
   `sheaf_doc::arlington` and the vendored model go. **Then the work
   between milestones pauses** until Greg says.
4. **The linearization crate** (Fast Web View, moved from M14): page 1's
   objects first, a linearization dictionary and hint tables, so a
   viewer reading byte ranges over a network shows page 1 before the
   rest arrives.
   - **Why a crate.** No Rust crate writes linearized files. lopdf
     0.46's `SaveOptions.linearize` is a flag that nothing acts on, and
     outside Rust, qpdf (C++) is the usual tool (build-or-adopt entry
     67). It's published as in-progress and instance-handoff were; see
     [Libraries](../libraries.md).
   - **Builds on object streams' grouping:** `serialize::group` (pure:
     each object's number and the pages that use it in, containers out,
     page 1's apart), which moves into the crate, and
     `materialize::graph::page_uses`, the bounded walk sheaf will
     describe its graph's page uses with.
   - **Independent of any object model,** sheaf's or lopdf's, in three
     steps:
     - **Plan:** the caller describes the graph (each object's id, what
       it refers to, which pages use it); the crate returns the order,
       the new object numbers and the object-stream groups.
     - **Serialize:** the caller writes each object with those numbers.
     - **Assemble:** the crate writes the linearization dictionary
       (padded to a fixed width, so the offsets settle in one more
       pass), the hint tables and the first page's cross-reference
       section.
   - **A checker** says whether a file is linearized and still holds:
     the full check (the hint tables, the first page's section), and a
     test oracle beside qpdf's. B3's minimum, Annex F's `/L` against the
     file's length, is in the lazy reader since object streams' slice 1.
     It wants the whole dictionary within the first 1,024 bytes (Annex
     F), where qpdf wants only its start there: the checker decides
     which on purpose.
   - **Written from ISO 32000-1 Annex F,** with qpdf (Apache-2.0) as
     the reference and the oracle, not translated, so the crate stays
     MIT OR Apache-2.0 with no notices for code taken from qpdf.
   - **A lopdf adapter** could fill lopdf's empty flag upstream.
   - **In sheaf, a writer option beside object streams,** used by a
     full rewrite to a new file: a "Fast Web View" choice in Save As,
     `--linearize` on `sheaf edit -o`, and later M14's Optimize.
   - **Not an edit and not a transform plugin.** Any later byte change,
     an incremental save included, voids it, so it can't live in the
     document. sheaf's own writer already has the object graph and each
     page's objects. A bytes-in, bytes-out plugin would have to parse and
     write the whole file again.
   - **No "keep it linearized" preference:** that would make every save
     of such a file a full rewrite, which breaks signatures and gives up
     the incremental save.
   - **Signed files** get the usual ask, since a rewrite breaks their
     signatures.
   - **Checked** with `qpdf --check-linearization`. Perhaps also with
     PDFium's availability API (`FPDFAvail_*`): page 1 renders from the
     file's first part alone. Whether pdfium-render binds it is
     unverified.
5. **F15: a Rewrite that doesn't load the whole file.** Every Rewrite
   of a large file pays for the eager load (Export Flattened peaks at
   513 MB on the heavy 7k file), and it's the same Rewrite path as 2
   and 3.
6. **`sheaf search`** over files and folders: the CLI half of
   [Search across files](../pending.md#search-across-files) (Pending). It's small:
   each file opens as a `Session` and runs `Text::search`.
7. **Roles and labels on sheaf-ui's widgets:** M17's first slice,
   pulled forward. Every dialog built after it inherits them, and every
   dialog built before it has to be retrofitted; M10 and M11 add many.
   The rest of M17's app half stays in M17.

After these, in order (2026-10-23):
1. **M10 slices 6–17:** appearances, filling, flatten, form data.
2. **Fill & Sign,** M12's slice 1, ahead of the rest of M12
   ([M12](../m12-signatures/README.md) says why).
3. **M11,** security and redaction. It needs M10's flatten (slice 15), and it
   has the most preparer value left.
4. Then M16, M10 slices 18–29 and the rest, in an order still open.

PDF/A conversion (M14) and tagging PDF/UA documents (M17) waited on the
content interpreter, which landed in #50. PAdES signing (M12) is
deferred until RustCrypto's cms 0.3 round (build-or-adopt D2).
