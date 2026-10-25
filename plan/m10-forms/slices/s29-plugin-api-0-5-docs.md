# M10 slice 29: Plugin API 0.5, and docs

Status: open · Part of [M10: Forms](../README.md)

**Plugin API 0.5, and docs** (proposal 23).
- **Field ops join the JSON `Op` as `Cap::Plugin`, with no WIT
  bump** (a plugin's fill goes through the same calculation as the
  user's; a flatten needs approval, as `FlattenAnnotations` does).
- **Field reads are new WIT functions in `sheaf:plugin@0.5.0`,**
  behind a manifest `fields` capability that also unlocks widgets in
  `annotations()`, `TextOpts { fields }` and `FormDraw::Appearances`
  for renders. Without it, plugin renders use `FormDraw::Redacted`.
  0.5 replaces 0.4: no 0.3 or 0.4 is served (one world, exact-minor
  until alpha; M10.5 proposal 18). SDK helpers; `docs/plugins.md`.
- User docs, `docs/forms.md`: filling, what scripts may and may not
  do, the save rules (XFA, usage rights, full rewrites, signed
  files), flatten, data formats (the JSON format and its version
  rule), detection, and the limits.
- README: the Forms rows; the Flatten and Clean Copy rows; the Save
  model row (replaced values, XFA, usage rights, signed files);
  Architecture and Key decisions (`sheaf-formjs`, the `formjs/`
  guest, boa as a wasm guest); Building and Packaging (the guest,
  wasm32-wasip2); Configuration (`[forms]`); keys; `sheaf form`.
- `docs/document-model.md`: form state. `docs/prior-art/README.md`:
  A's "Revisit at M10" settled.
- ROADMAP: M10 marked done, and Pending entries for what Not in M10
  leaves Pending or for later. (`m11.md`'s plugin API is already 0.6,
  since M10.5 slice 20.)
- **Checks:** a plugin without `fields` sees no widget, no field text
  and redacted field values in renders; with it, all three; a
  plugin's fill lands as the user's would, calculation included.
