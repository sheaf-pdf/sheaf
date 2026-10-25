# M10.5: M10 slices 6+ on the new API

Part of [M10.5: the edit API](README.md).


M10 resumes on `main` after **all** of M10.5 (slices 1–20; proposal 11).
Slice 20 rewrites `m10.md`'s slices to match this list.

- **6, text appearances.** A pure
  `appearance::text(field, widget, display, &FormDoc, font) -> FormRes`
  on ContentWriter and `Font`. It covers the `/DA` parser; WinAnsi,
  MacRoman and `/Differences` encoders as `Font`s; `/DR` handles from
  `SourceForm`; `/MK /R`; `WidgetProps.border`; and `/Tx BMC` with a
  clip. The emitter calls it for widgets whose appearance digest
  changed, and so does the realizer, so the screen equals the file. The
  overlay-or-pictures spike is settled.
- **7, the Unicode fallback.** `Res::Font(Fallback)`: a GID-preserving
  Noto Type0 subset, late-bound per materialization or overlay. An
  Append adds a new subset object when new glyphs appear. NFC, MaxLen by
  grapheme and the refusal live in `SetFieldValues`' lowering. It ends
  the `'?'` interim in marks and generators.
- **8, buttons, radios and choices.** Pure generators. Group `/AS`
  semantics are lowering rules of `SetFieldValues`.
- **9, saving values. It shrinks to one commit.**
  `SetFieldValues { values (by FieldId or full name), display, calculated }`
  lowers to `Put Field`/`Put Annot`. Both strategies, recovery and
  identity after a save already exist, so 9a's re-binding is deleted.
  The save rules are not per-writer code:
  - a replaced value is REPLACES_TEXT, so a Rewrite;
  - filling an empty field appends, keeping UR3 within its
    TransformParams;
  - earlier-revision values are a `SourceForm` fact that forces a
    Rewrite on the first form change;
  - XFA (tooltips to `/TU`, after `XfaHoldsValues`), `/UR3` and SigFlags
    are `rules::plan` transforms;
  - signed and certified files go through §9.3's asks, with "Append
    anyway";
  - lowering refuses edits to fields locked by a known signature
    (FieldMDP), and to signed fields themselves, with the
    `EditError::Locked { field }` this slice adds;
  - the encryption notice is gone, because encryption is kept.

  Check: a fill on a signed hybrid fixture offers "Append anyway" and
  doesn't rewrite unasked.
- **10, focus, keys and Tab order.** Hit tests on PageSpace widgets in
  `page_annots` order. **It adds `KeyPatch` with symbolic `PatchValue`s,
  and `PageDef.keys` for `/Tabs`, with a classify role row (`/Tabs` →
  FIELD_TREE, OTHER).** The `/S` order comes from kept.rs's structure
  walker over source facts. Pending UI commits flush before any plugin
  call.
- **11, the field editor. It adds `GroupKind::Typing`:** one group per
  focus session, each commit a `SetFieldValues` amend. The editor's text
  stays uncommitted, with its own Ctrl+Z. `ViewInputs` hides the focused
  widget from the realization while the editor draws.
- **12, drawing changed fields.** It becomes "marks drawn by PDFium":
  switch to `MarkDraw::ByEngine`; delete the GPUI mark painter
  (`annot_elements`), `stamp_preview.rs` and the tape pictures; and
  decide FFLDraw (recommended: appearances only, once every changed
  field has an `/AP`).
- **13, buttons and Clear Form.** `ResetFields` as one Tx. Hide is
  `PatchWidgets { flags }`.
- **14, the Fields panel, and values in search.** Model reads;
  `TextOpts { fields: true }` merges widget display text, for the app.
- **15, flatten.** `FlattenFields` shares `FlattenAnnotations`'
  machinery:
  - ensure appearances;
  - draw printable widgets by `/AS`, with no first-state fallback, into
    a `Placements` layer of `FormRef::Pinned`;
  - remove widgets through `widget.field`;
  - empty the `FormDoc`;
  - pass `removed_annots` to `keep_only`.

  **It adds `FormBring::Flatten`** for Insert and Combine, and
  `SignatureNotValidHere` fields may flatten there. Also
  `sheaf form flatten`.
- **16–17, form data.** FDF, XFDF, CSV, text and JSON importers produce
  `SetFieldValues` batches by name, resolved through calculation and
  committed `Validate`. Exporters are queries.
- **18, AF functions.** Pure Rust, in the host's resolve stage. They
  produce display and calculated values plus their decision reads.
- **19–22, scripts. They add `Actor::FormScript` and
  `PageSource::Template`** (spawning pages from templates).
  `FormsHost::resolve(&Snapshot, Vec<Op>) -> (Vec<Op>, reads)` runs on
  the formjs worker, then commits `Validate` into the user's Typing
  group. The sandbox syncs on `ChangeSet.fields` for transitions not
  made by `FormScript`. Replay never runs scripts, and a save waits for
  a running cascade's group.
- **23–26, Prepare Form.** `AddField`, `AddWidget`, `PatchFields`,
  `PatchWidgets`, `MoveWidgets`, `DeleteFields`, `DeleteWidgets`,
  `RenameField`, `MoveField`, `SetTabOrder` and `SetCalculationOrder`,
  over existing keys. **They add `FieldNode.patch`, widget patches
  (`PatchValue`) and role rows** (removing `/AA` or `/JS` → REMOVES;
  `/AP` on a widget → FILL). Multi-select is one Tx with locals.
  Deleting is REMOVES, so a Rewrite. Copies are untagged until M14.
- **27–28, detection.** Queries over the TextService and the pool.
  Accepting the proposals is one Tx.
- **29, the plugin API.** Field ops join the JSON `Op` as `Cap::Plugin`,
  with no WIT bump. Field reads are new WIT functions in
  `sheaf:plugin@0.5.0`, behind a manifest `fields` capability that also
  unlocks widgets in `annotations()`, `TextOpts { fields }` and
  `FormDraw::Appearances` for renders. Without it, plugin renders use
  `FormDraw::Redacted`.

**Edits to `m10.md`'s proposals and slices** (slice 20):
- proposal 8: delete its "Re-bound after a save" bullet and the dirty
  tracking of saved values;
- proposal 10: replace the on-screen slot-hiding bullet with
  `ViewInputs`;
- proposal 7's "every way out": point it at `rules::plan` plus the
  export preflight;
- proposal 18: becomes `FlattenFields`/`FlattenAnnotations`;
- proposal 23: renumbered to 0.5, with no 0.3 or 0.4 serving;
- slice 9: drop 9a's re-binding;
- slice 11: the Typing group;
- slice 12: "marks drawn by PDFium";
- slice 15: `PinnedAp`;
- slice 29: 0.5 with `fields`, with no 0.3 or 0.4 serving; `m11.md`'s
  plugin API becomes 0.6; `docs/forms.md` drops "encryption until M11"
  from the save rules (m10.md:1817–1834).
