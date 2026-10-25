# M10.5: Proposed (to confirm at kickoff)

Part of [M10.5: the edit API](README.md).


Greg's standing instruction is to take the recommended answer unless he
objects, so each of these stands as written until he does. Proposals
8–10 are open questions for him, marked below and listed first under
Asks.

8. (Open for Greg.) **Header/footer page numbers are live; Bates numbers
   and the date and file tokens are frozen at apply.**
   - `<<1>>`, `<<N>>` and `<<label>>` are re-evaluated in every output:
     the binder after a reorder, an extract, a print. The page says what
     it is, and exports and prints match.
   - The cost: a reorder after a save re-lays out every covered page. On
     a 7,000-page document with "Page n of N", the next Append writes up
     to 7,000 small block streams and 7,000 page dicts (a few MB), and
     the realizer re-renders those pages. That cost is bounded and
     measured (slice 17's live-token reorder gate: at most 8 MB and 1 s
     for the Append).
   - The machinery it needs: `CtxNeeds` positions in render keys,
     `LIVE_REDRAW`, `SaveRecord.drawn`, and allowed drops in the removal
     check.
   - The alternative is frozen numbers plus an "Update page numbers"
     action, as in Acrobat. A reorder then writes one `Order` value and
     appends only `/Pages` nodes, and the machinery above goes; but the
     numbers are stale until the user updates them, and extracts keep
     the binder's numbers.
   - Bates numbers and the date and file tokens are frozen either way.
   - If that gate fails on the heavy fixture, fall back to frozen
     numbers plus Update. Slice 17's notes record the result as this
     proposal's verdict.
   - **Verdict (slice 17; Greg, 2026-10-22): live, not the fallback.**
     The Append passes in the session that saved the header; the
     Append on the file opened again and the first draw of a group none
     drew before miss, and Greg accepted the misses rather than freeze
     the numbers. A lead window (option A: the visible pages realized
     ahead of their group) is to revisit. Slice 17's notes have the
     numbers.
9. (Open for Greg.) **Replacing text that is already in the saved file
   forces a full rewrite** (strict removal).
   - It covers mark text, a bookmark title, a label prefix, a field
     value, OCR words, header or watermark text, and anything else
     already in the saved file. Today only removals force one.
   - It extends M9's "deleting has to delete" and M10 proposal 7's "a
     replaced value forces a full rewrite" to every replaced text, and
     the removal check (§11.9) backs it up.
   - The cost: a note edit or a bookmark rename after a save becomes a
     Rewrite (about 0.5 s on the synthetic 7k, more on large scans), and
     signed files ask more often. "Append anyway" is offered on them,
     keeping the old text in the earlier revision.
   - A regenerated page number is the one exemption. Geometry and style
     changes append.
10. (Open for Greg.) **Exported copies keep the source's encryption by
    default.**
    - It covers Extract, Split, Export Flattened and Clean Copy. Today
      exports are unencrypted, because loads strip `/Encrypt`.
    - A password-protected client file shouldn't produce unprotected
      copies silently.
    - Print and page images stay unencrypted bytes in memory either way.
      Save As is a Save, so it always keeps the encryption.
    - No checkbox per dialog in M10.5. M11's security UI adds the
      choice.
11. **M10 resumes only after all of M10.5 (slices 1–20).**
    - It keeps Greg's sequencing (decision 2): M10's slices 6+ are built
      on the finished API, and no M10.5 slice trails.
    - Slice 20 rewrites `m10.md`'s slices 6+ to match (M10 slices 6+ on
      the new API, below).
    - M10's slices 6–9 shrink by about one slice: the spike, the
      re-binding and the per-writer save rules go.
12. **History stores resolved primitives with their before-values, and
    the snapshots stay.**
    - Replay applies the prims and checks each before-value. It never
      re-runs lowering, so it can't drift when an input nobody recorded
      changes (bug 6), and a mismatch is reported, never a silent no-op.
    - Before-values cost nothing (they are the parent's Arcs) and buy
      `classify` from a step alone, the commit-time value check, and
      describe text for pruned parents.
    - One primitive, `Put`, over 11 key kinds keeps the set closed.
      Ordered collections are values, not splices.
    - Prior art: ProseMirror's steps fail rather than repair; Slate's
      operations carry their before-values; Lexical keeps full
      snapshots, as sheaf does.
    - Snapshots stay, so undo and jump stay O(1); decision 3 holds the
      snapshot undo guarantees fixed.
13. **Versions are immutable, and commits validate by value, with two
    stale policies, `Validate` and `Relower`.**
    - An amend replaces the version with a new id, so a version token
      alone says whether anything changed (bug 7).
    - Validating by value is exact under amends, ignores changes that
      cancel out, and doesn't care whether the base was pruned.
    - `Relower` covers work whose ids nobody saw yet (inserts, blank
      review) and plugin calls with pinned ids, so a user edit to an
      unrelated page doesn't refuse them.
    - Three-way merges of ordered keys, and OT or CRDTs, stay out (Not
      in M10.5).
14. **Page content is a definition in the model: a content stack per
    page, and generators for the document.**
    - The derived copy exists only to change content, the M10 worker's
      "strongest argument for page content in the model". With content
      in the model, the page object, annotations, fields and structure
      parents stay where they are.
    - A stack holds definitions, not bytes. Additions never re-encode
      the original content, so an Append stays possible, and a
      regenerated result is a cache, never the definition (Onshape's
      lesson).
    - History, plugins and the planner can tell a crop (geometry) from a
      watermark (a generator) from a removal (a filter).
15. **One emitter feeds every output, and a save is an output, never a
    rebase.**
    - The full rewrite, the incremental append, the recovery section and
      the render overlay are four scopes of one `emit`, so the two
      writers can't drift (bug 14) and the screen equals the file.
    - The model never re-expresses a version after a save (bug 1). A
      `SaveRecord` carries what the next save needs.
    - Stable numbering keeps object identity across saves, so a changed
      mark is rewritten under the same number.
16. **The screen renders edited pages from in-memory append sections
    over the source's bytes.**
    - Measured at 7,000 pages: the original plus a 370-byte update
      section loads in 2.0–2.4 ms, and the two-segment
      `FPDF_LoadCustomDocument` reader avoids the copy (2.16 ms).
    - PDFium's `GenerateContent` is lossy, takes about 95 ms per change
      and mutates the live document. Per-page scratch PDFs, per-source
      generations and per-page overlays were considered and rejected
      (§10.1).
    - Slice 2's spikes decide the group size, the encrypted-base path
      and the box-edit path by numbers before anything depends on them.
17. **The content lexer, splicer, writer and fonts move forward from M11
    into `sheaf-doc/src/content/`.**
    - Removing headers and watermarks, recognizing blocks after another
      tool merged streams, pruning resources and the removal check all
      need them now.
    - M11's interpreter consumes `Tok` spans, so nothing is thrown away.
      The semantic interpreter stays in M11 (§13).
    - It retires lopdf's `Content` round trip (f32 numbers, broken
      inline images) and four text encoders.
18. **Plugin API 0.4 is the only world, and ops cross WIT as JSON of the
    Rust `Op`.**
    - 0.2 and 0.3 are no longer served (decision 3). The manifest check
      is exact-minor until alpha.
    - JSON of the exact type means no hand mirror, and a new `Op` needs
      no WIT bump; only new read functions bump the minor.
    - `deny_unknown_fields`, per-kind refs with clear errors, the
      published schema and the `edit-schema` manifest check make up for
      the compile-time typing non-Rust plugins lose.
    - M10's field reads become 0.5, and M11's API 0.6.
19. **A plugin call is one undo step. A trap, Err or cancel rolls it
    back by truncating or reverting.**
    - When the user edits in between, the call's versions are fragments
      of one group, undone one at a time or all together.
    - Today an Err, a cancel or a trap drops every batch silently, and a
      call that succeeds makes one version per batch.
20. **Plugins never learn the user's identity.**
    - Authors are roles, and `<<name>>` and `<<initials>>` stay literal
      without the `identity` manifest permission, which no bundled
      plugin asks for.
    - Today `sign_marks` fills them in with the user's name, which the
      plugin can read back (bug 27).
    - Field values in plugin renders are gated by 0.5's fields
      capability (M10 slice 29).
21. **Older sheaf marks (`/SheafAnnot` v1) are converted on read.**
    - Geometry comes from the standard keys, which are already
      PageSpace; the JSON supplies only what PDF can't hold.
    - It's nearly free, which is decision 3's bar for migration code.
22. **Unencodable text gets one interim rule for marks and generators:
    `'?'` in what's drawn, the exact text kept, and a notice, until M10
    slice 7.**
    - Emit stays total, so a later label change can't make a document
      unsaveable.
    - M10 slice 7's Noto fallback ends it, behind the same `Font` trait.
23. **Encryption preservation is built into M10.5's serializer and
    savers (slices 7, 9 and 10), and M11 slice 1 shrinks.**
    - The new serializer encrypts per object anyway, and recovery for
      encrypted files depends on it.
    - It ends M11 bug 2 and the "password protection removed" notice.
24. **Copied widgets and annotations are untagged until M14.** Pages
    copied, or inserted from other structure trees, lose
    `/StructParents`, with a Note when the source was tagged.
    - Keeping `/StructParents` on a copy would point into the original's
      parent tree. Modeling structure is M14's.
25. **Two cascade changes: a field whose last modeled widget is deleted
    is removed, and a label range whose first page is deleted re-anchors
    to the next page of its range.**
    - The first makes the model match what `keep_only` already writes.
      The model test `form_fields_follow_their_pages` (model.rs:3055;
      the assert at :3080) inverts.
    - The second keeps a range's numbering when its first page goes;
      today label anchors aren't validated at all.
26. **OCR text is untagged invisible text, with a private marker carried
    by a named property list.**
    - A named property list lets a merged stream still classify, and
      keeps Strip a dict edit.
27. **Crop is box-only.** Removing the content outside the box is M11's.
    - A crop then appends with only page dicts in the section.
28. **Edits are disabled until the open's facts are read.** There are no
    late facts: a slow file just waits longer.
    - Today six loaders drop facts that arrive after the first edit (bug
      10).
29. **The stamp library stays outside the edit API.**
    - Its files are the user's own library, not document content.
      Placing a stamp interns its form.
30. **Watermarks stay upright after a rotate, like headers.**
    - Layout happens in the displayed frame at emit, so a dialog's
      preview equals the result. Today a watermark's orientation is
      fixed at apply, and a header's at save.
31. **sheaf replaces Acrobat-added headers, footers and watermarks of
    the same kind automatically, and never removes authored ones (Word,
    InDesign) without `include_authored`. Dialogs show the counts.**
    - Acrobat's are recognized by their `/PieceInfo`
      `ADBE_CompoundType`. Anything else marked `/Artifact /Pagination`
      is an author's running head.
32. **Save As of a signed document copies the file and appends, so its
    signatures survive. Save As of an unsigned document rewrites.**
    - That is what Acrobat does.
33. **A file changed on disk always asks: Save As…, Overwrite or
    Cancel.**
    - Today a changed length silently falls through to a rewrite over
      the other program's changes (bug 2).
34. **UR3 is kept on Appends within its TransformParams, and dropped on
    every Rewrite.**
    - Reader validates later revisions against `/TransformParams`, so a
      permitted fill or comment keeps Reader's rights. A Rewrite always
      breaks UR3's signature.
35. **Combine opens the first file, makes one transaction, saves to the
    chosen path, and hands the document to the new window.**
    - Its notices and the scripts-off state then follow the document;
      today they can't reach the window Combine opens. The first file's
      catalog keys flow through as today.
36. **The agent socket is out of M10.5.**
    - Nothing in M10.5 produces or uses it. The agent milestone brings
      its own transport (§20).
37. **Object-stream packing moves to M14, and `keep_only` is not ported
    to a symbolic graph.**
    - Neither has an M10.5 producer. `keep_only` is ported only if slice
      9's Rewrite gate fails.
38. **M12's hooks are dropped from M10.5.** Reading revisions stays, for
    forms' EarlierValues.
39. **PDF/A and PDF/UA claims get a Note when a change breaks them.**
    Conversion and font embedding are M14's.
40. **The CLI:**
    - `--force` keeps meaning "overwrite" only, as in every other
      subcommand;
    - signature and XFA answers are explicit flags;
    - exit code 3 means a decision is needed, with the `Ask` as JSON, so
      a script never has a choice made for it silently;
    - `sheaf edit` addresses by position, title path, `/NM` and `--with`
      names;
    - `sheaf inspect --json` is added.
