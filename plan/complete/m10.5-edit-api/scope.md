# M10.5: Scope

Part of [M10.5: the edit API](README.md).


- **The model:** content-addressed, lazily read sources and a blob store
  (§3.1); `DocState` holding page content, other tools' annotations,
  widgets, the field tree with explicit form instances, the outline and
  generators as records (§3.2–3.9); one coordinate frame below the page
  (§5).
- **The vocabulary:** a new `sheaf-edit` crate of about 25 public ops
  that lower to `Put { key, before, after }` over 11 key kinds;
  transactions with locals, typed errors, named cascades, host-owned
  attributes and capabilities (§6).
- **Effects and history:** a pure `classify`; versions that name
  immutable states; checked replay; groups for OCR and plugin calls;
  value-validated commits with two stale policies; a byte budget that
  prunes (§7, §8).
- **Saves:** saves as outputs with a `SaveRecord`; a planner with
  signature impact and "Append anyway"; in-place appends with a journal;
  a removal check that fails closed; encryption kept; recovery v2 (§9,
  §11).
- **Rendering:** a realizer that renders edited pages from in-memory
  append sections, one engine pool, render keys, view inputs and a text
  service (§10).
- **Exports** as transforms on an export transaction (§12), and the
  content lexer, writer and fonts pulled forward from M11 (§13).
- **Consumers:** one commit door and one refresh in the app, one open, a
  `Session` for headless use, `sheaf edit` and `sheaf inspect`, and
  plugin API 0.4 as the only world (§14).
- **Moved out:** see Not in M10.5. Nothing is built for the agent, but
  nothing in the API needs reshaping for it (§14.4).
- **Size:** 20 slices, about 68 points (S = 1, M = 2, L = 3, XL = 5).
