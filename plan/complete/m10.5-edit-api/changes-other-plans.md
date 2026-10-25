# M10.5: Changes to other plans

Part of [M10.5: the edit API](README.md).


Slice 20 makes these edits, except where a line says otherwise.

- **[`m10.md`](../../m10-forms/README.md):**
  - slices 6+ rewritten to match the list above, with the edits to its
    proposals and slices listed there;
  - the Status line says M10 resumes after M10.5;
  - proposal 22 and the Settled-at-kickoff line "M10 only warns" about
    encryption: moot, because every save keeps encryption from slice 7
    on (proposal 23); the warning goes, and so does proposal 7's "On an
    encrypted file, until M11, only Rewrite is offered" (m10.md:446).
- **[`m11.md`](../../m11-security/README.md):**
  - **Slice 1 shrinks** to notices, Properties, owner-only files, lopdf #584,
    the certificate refusal and temp cleanup (the attachments
    folder). Encryption kept on every save (its proposal 10) lands here,
    in slices 7, 9 and 10.
  - **Its bugs:** 2 (encryption dropped) and 11 (the incremental writer
    can't encrypt) are fixed here (slices 3 and 7); 4 (no recovery for
    owner-only files) by the encrypted sidecar (slice 10); 5's generated
    cache and recovered temp file (slices 9 and 19), leaving the
    attachments folder to M11; 6 (Patterns searches only the primary) by
    the text service (slice 14); 9's watermark settings and local image
    path are no longer written (slice 16), and Clean Copy strips
    `private::KEYS` (slice 19); 10's lopdf #603 is closed by the lazy
    reader and `serialize/crypt.rs` (slices 3 and 7). Bug 12
    (`PdfFile::open`/`save_as`, file.rs:10) is untouched by M10.5. Bug
    13 (the only content rewriter is narrow): `stamps::strip` is deleted
    (slice 17), and the lexical splice in `content/` (slices 6 and 16)
    is what M11's rewriter builds on.
  - **Slice 3** (the content interpreter) builds on `content/` (lex,
    splice, `Balance`, TextBag; §13), adds `ContentStack.rewrite` and
    `Static::Ops`, and lowers redaction as definitions evaluated at emit
    (§20).
  - **The leak check** (its proposal 8) runs over the output, not
    DocState (§20).
  - **Applying redactions** is a `Transform` on an `ExportTx`, so
    plugins can't apply them by type; "every way out asks first" is the
    export preflight (§12.1).
  - **Its plugin API bump** (proposal 15) becomes 0.6, since M10 takes
    0.5.
  - **Its proposal 11** (no plaintext copies on disk) is mostly met
    here: generated sources are memory blobs, and the sidecar is
    encrypted.
  - **New AES-256 encryption** (V5, R6) writes `/Length 256` in
    `/Encrypt`, in M11's encryption code or as a `vendor/lopdf` patch:
    lopdf's `Document::encrypt` leaves it out, and poppler 26.01 then
    garbles every stream. Its tests check new AES-256 output with
    poppler (M10.5 slice 2's notes).
- **[`agent.md`](../../m16-agent/README.md):**
  - "Submit edits" (agent.md:129): sheaf-edit `Op` JSON in a versioned
    envelope, through `Document::plan(actor, base, ops) -> PlanHandle`
    and `commit(plan)`, so the approval card commits exactly what it
    showed (§20).
  - "Versions keep reads and writes honest" (:133–136): version tokens
    are `{ instance, version }`; the agent's reads are decision reads
    kept per MCP session, and it commits `Validate`, so a user edit
    elsewhere doesn't refuse it and it can write during OCR; plans
    expire on conflict.
  - The History row (:127): `VersionView` with stored lines, not
    `describe.rs` at read time; `Document::edge`'s source list is gone.
  - The author "Claude for GE" (:166–168): `Actor::Agent` is session
    provenance; nothing goes into the file except a mark's `/T`, which
    the host sets.
  - "Revert this turn" (:169–171): a revert of the turn's versions; a
    jump only when nothing came after the turn.
  - Its own transport and approval UI, not the sheaf-instance socket
    (proposal 36); semantic ops only at first (decision 6); field values
    behind its per-document consent.
- **[`file-contention.md`](../../topics/file-contention.md):**
  - §3 Merge (file-contention.md:147–163): a version's steps now hold
    resolved ops and prims with before-values, so a merge applies them
    onto the other person's version by value (`Validate`) or re-lowers
    the resolved ops (`Relower`), and conflicts are explicit. Marks
    still map by `/NM`. Sheaf marks' `/NM` embeds the `AnnotId`, so it
    is unique per document (duplicates get fresh ids); others' `/NM` is
    unique per page only. `Document::edge`'s source list is gone, since
    sources are content-addressed.
  - "Someone else saved" is detected by the `FileStamp` re-check before
    every write, which asks Save As…, Overwrite or Cancel (proposal 33).
- **Plugin docs:**
  - [`docs/plugins.md`](../../../docs/plugins.md) is rewritten for 0.4 with
    the generated schema: the `Batch` example (:53–61) becomes the `Ops`
    builder; "Batches are applied in order after your call returns"
    (:204) becomes one transaction per call, `submit` returning ids, and
    `commit()`; the edits list becomes the `Cap::Plugin` ops; and the
    versioning policy is "one world, exact-minor manifest check,
    `edit-schema`" until alpha.
  - [`plugins.md`](../m06-plugins.md)'s "the host keeps N-1 worlds" (:108,
    :111) is superseded by that policy.
- **[`ROADMAP.md`](../../../ROADMAP.md),** at kickoff:
  - an M10.5 row in the milestones table, with no release of its own;
  - an M10.5 section between M10 and M11 that points here, with its
    slices line;
  - M10's "Paused after slice 5" paragraph (ROADMAP.md:227) points here
    and says it resumes after M10.5;
  - M11's "Preserve encryption on save" bullet moves to M10.5, and its
    plugin API becomes 0.6;
  - the parity table row "Set / remove passwords, permissions; preserve
    encryption | M11" (ROADMAP.md:407) splits: "preserve encryption"
    moves to M10.5, and passwords and permissions stay M11;
  - at the end, M10.5 marked done.
- **[`README.md`](../../../README.md),** in slice 20's docs pass: the Save
  model row (README.md:152: replaced text forces a rewrite too, signed
  files append or ask, a file changed on disk asks); "saves write a
  plain (unencrypted) file" (:275); plugin API 0.3 (:372, :522) to 0.4;
  the recovery description (:574) to v2, encrypted.
- **[`docs/document-model.md`](../../../docs/document-model.md):** each slice
  rewrites its own section; slice 2 adds the spike verdicts; slice 20
  makes the final pass, including the invariants section (each invariant
  naming its test) and the outdated lopdf-incremental and pure-function
  claims (bug 30).
