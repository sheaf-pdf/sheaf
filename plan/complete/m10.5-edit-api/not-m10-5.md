# M10.5: Not in M10.5

Part of [M10.5: the edit API](README.md).


- **The semantic interpreter, glyph boxes, descriptors, removal
  transforms, a hard crop, the leak check:** M11.
- **The security dialog, new encryption, a permissions UI, the
  certificate refusal, `Key::Security`:** M11. (The serializer's
  encryption is here.)
- **Signing, reservations, byte-range holes, a diff per revision,
  `SigClass` SIGN and LTV, DSS:** M12.
- **Object editing, link bodies, key patches for annotations,
  `Source::users`:** M13. (The `KeyPatch` type lands in M10 slice 10.)
- **Object streams in written files, structure modeling, tagging copies,
  an embedded-font policy, PDF/A conversion, Save Optimized:** M14.
- **Batch steps, `BatesStart::Continue`, `PageSel::{Label, Matches}`:**
  M15.
- **The agent:** its transport, approval UI, `Actor::Agent` and plan
  handles are its own milestone.
- **`GroupKind::Typing`, `Actor::FormScript`, `PageSource::Template`,
  `FormBring::Flatten`, field ops, `FormDraw::Redacted`:** M10 slices
  11, 15, 19–22 and 29.
- **Joining an existing form instance on insert (`FormBring::Join`):**
  when a producer appears.
- **Three-way merges of ordered keys:** not planned; `Relower` covers
  it.
- **Persisted history, merge, OT or CRDTs:** not planned. Steps are
  serde-capable.
- **hayro as an engine:** not planned; an oracle only.
- **Shrinking `/SheafAnnot` to the keys PDF can't hold:** a follow-up.
- **The stamp library in the edit API:** not planned (proposal 29).
- **Linearization, and a balanced re-tree on Append:** later, if corpus
  tests ask for them.
