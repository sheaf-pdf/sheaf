# M10.5 §20: What later milestones build on, and the rules recorded for them

Part of [M10.5: the design](README.md).


**M11 (redaction, sanitize, security):**
- **Slice 1 shrinks** to notices, Properties, owner-only files, lopdf #584,
  the certificate refusal and temp cleanup. Encryption
  preservation already landed in slices 7, 9 and 10 here.
- **`SetSecurity`:**
  - `Key::Security` holds the spec with secret handles: `Zeroizing`,
    redacted in `Debug`, and excluded from `Step.op` JSON, describe
    lines and serde.
  - Binder saves encrypt with the document's security (`FromDocument`).
    `Profile.encryption = New(spec)` is for exports only.
  - `EncryptionChanged` forces a Rewrite, and recovery is written in
    Full form, encrypted with the target security.
- **The interpreter** builds on `content/` (lex, splice, `Balance`,
  TextBag).
  - `ContentStack.rewrite` is "the new base": setting it absorbs the
    current filters (cleared), and later filters apply to its output.
  - `Static::Ops` and redaction output are **definitions**
    (`Rewrite::Redact { areas, policy }`, `Rewrite::Edit { ops }`),
    evaluated per page at emit with a bounded digest cache, as
    `SymObject::Deferred` bodies.
  - `ApplyRedactions` and `Sanitize` take a `PageSel`, so Preview lowers
    only visible pages. Lowering reports progress per page.
  - Partial-image redaction decodes through PDFium in the host resolve
    stage (§6.3).
- **The leak check** runs over the **output**: a walk of the Emission
  (Built values plus Raw source objects reachable from the roots), with
  content and appearance strings decoded, plus the text service over the
  materialized bytes. It covers Info and XMP, embedded-file names, named
  destinations, link URIs and Launch targets, structure `/Alt` and
  `/ActualText`, `/TU`, JavaScript, `/PieceInfo` and `/SheafAnnot`. A
  DocState pass (`modeled_text`) is only a fast pre-check.
- **Plugins never apply redactions,** because `ApplyRedactions` is a
  `Transform`. If apply-in-place arrives, it becomes an `Op` with
  `Cap::User`.
- **Plugin API:** M10 takes 0.5 and M11 takes 0.6.

**M12 (signatures):**
- **Signing is two ordinary commits around one Append:**
  1. `SignField { field, appearance, reason, mdp: Option<DocMdp>, lock: Option<FieldLock> }`
     lowers to Puts, so locks and DocMDP are model data that lowering
     enforces.
  2. The save runs with `Profile.reserve` (a zero-filled `/Contents`
     hole and a fixed-width `/ByteRange`). `serialize/` returns the
     hole's offsets.
  3. After the CMS is injected,
     `SetSignatureValue { field, contents: BlobRef, byte_range }`
     commits, and `record_save` attaches the record to that version, so
     `emit(state)` reproduces the file byte for byte.
- **Signing requires `plan == Append`.** Otherwise the user saves first,
  through its asks, then signs.
- **`SaveRecord.signatures`.** `SigImpact`, lock checks, the signature
  panel and revisions use `Source::signatures()` ∪ the saved chain's
  signatures.
- **`SigClass` gains SIGN and LTV.** DSS and document timestamps
  classify as LTV, which PAdES allows after P1, and `Key::Dss` is added.
- **"Changes since signing"** uses a content-based role classifier over
  the objects of later revisions (`/Type /Annot`, `/Subtype /Widget`,
  `/FT`, `/Sig`, `/Page`, catalog keys). The `ObjKey` mapping is only a
  test oracle for sheaf's own Appends.
- **Already in place from M10.5:** Save As copy-and-append,
  `Source::revisions()` and the Append serializer.

**M13 (object editing):**
- `AnnotBody::Link { rect, quads, target: Target, border, h, src, copy }`.
- `KeyPatch` role rows and `PatchValue` targets for link `/Dest` and
  `/A` edits (the type itself is M10 slice 10's).
- `Static::Image { blob, matrix }`.
- An object query that unifies base items and placed layers, and a
  content hide set in `ViewInputs`.
- `Source::users` and the in-model users index arrive with the first
  "all uses" op.

**M14 (optimize, PDF/A, accessibility):**
- **Object streams.** Packing in Rewrite and Append, with
  `packable(ObjKey, &SymObject, gen)` encoding ISO 32000-1 7.5.7's
  exclusions: streams, generation ≠ 0, `/Encrypt`, an ObjStm's own
  `/Length`, xref streams, and signature holders. Member strings are
  never encrypted individually; the container is.
- **Fonts.**
  - `Profile.fonts: Std14Ok | EmbedAll` is part of the `appearance()`
    and `layout()` digests and of `Res::Font` resolution.
  - Metric-compatible substitutes (Liberation) sit behind `Font`.
  - Foreign non-embedded fonts need an all-uses patch.
- **PDF/A claims.** Never write transparency into a PDF/A-1 source, and
  offer to remove a claim explicitly. M10.5 only adds the Note.
- **Structure.** `Key::StructElem`, `/ParentTree` emitted from the
  model, and `PlaceTag::Struct(elem)` for flatten. These replace M10.5's
  "drop `/StructParents`" rule.
- **Save Optimized** writes an export to the path, then reopens it as a
  new session, with an explicit "this clears undo history" notice.
  Alternatively, only Save As Optimized is offered.

**M15 (batch):**
- **A versioned step schema** above `Op`:
  `Step = Op(Op) | Job(JobSpec { Ocr, TrimWhite, RedactPattern, … }) | Export(Preset, Vec<Transform>)`.
  A host batch runner executes it as `Actor::Batch` with the user's
  caps.
- **`BatesStart::Continue`**, resolved by the runner, and
  `PageSel::{Label, Matches}`.

**The agent milestone:**
- **Transport and entry points:**
  - its own transport and approval UI;
  - `Actor::Agent`;
  - `Document::plan(actor, base, ops) -> PlanHandle` (a lowered `Tx`
    plus a `Preview`), and `commit(plan)`, so the approval card commits
    exactly what it showed;
  - plans expire on conflict.
- **Reads.**
  - The agent's reads are decision reads, kept per MCP session, and it
    commits `Validate`, so it can write during OCR.
  - Field values sit behind [`agent.md`](../../../m16-agent/README.md)'s per-document
    consent.
- **"Revert this turn"** is a revert of the turn's versions. A jump is
  offered only when nothing came after the turn.
