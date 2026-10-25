# M10 proposal 20: Where the code goes

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**Where the code goes.**
- `sheaf_doc::forms`: reading, the model's types, appearances, the
  save rules, flatten, and the Rust AF functions.
  `sheaf_doc::exchange` gains form data.
- `sheaf-formjs` (new, GPUI-free): the wasmtime host, the worker
  thread and the event protocol, behind `forms-js` (decision 3). The
  CLI and plugin edits get calculations through it.
- `formjs/` (new): the guest (boa, pdf.js and sheaf's glue), built
  for wasm32-wasip2 in its own workspace by the script that builds
  the bundled plugins (`scripts/build-plugins.sh`), and shipped
  beside them.
- `sheaf_ops::forms`, and `sheaf form` in the CLI.
- `FieldEditor` in sheaf-ui.
- In the app: filling, Prepare Form and the Fields panel, each a
  module of its own.
- gpui-pdf's `forms.rs` (MIT), which `rust-ecosystem.md` suggests
  borrowing, is read as a reference, not vendored: it saves by lopdf
  full rewrite (rust-ecosystem.md:147).
