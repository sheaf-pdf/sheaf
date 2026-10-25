# M11: Not in M11

Part of [M11: Security & redaction](README.md).


- **Certificate encryption:** after M12 (decision 3).
- **Applying in place:** not now (decision 4); the same engine can add it
  later, as an `Op` that needs `Cap::User` (M10.5 §6.7).
- **AI or PII detection** (Foxit Smart Redact, Acrobat's suggestions).
  The agent panel (Pending, [`agent.md`](../m16-agent/README.md)) could suggest
  redaction marks later; it would place them, not apply them.
- **Redacting a whole folder:** M15 batch actions, on top of `sheaf
  redact`.
- **Re-subsetting fonts** so that glyphs used only in removed text drop
  out of the font program: M14 optimize. Until then a font subset can
  show that a character occurred, though not where. Documented.
- **Overlapping objects** (Acrobat's sanitize category): ❓ for M14.
- **Rights management** (Microsoft Purview / MIP, Adobe AEM, Foxit RMS),
  security policies, FIPS mode: not planned. They're enterprise services
  in the same class as the cloud features.
- **Redaction code sets** (FOIA, Privacy Act). Overlay-text presets cover
  what tax and audit work needs.
