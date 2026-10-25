# M5 proposal 20: Separation is core; so is intake. Plugins see the results

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**Separation is core; so is intake. Plugins see the results.**
- Scanner APIs are native (C, COM, Objective-C), which wasm plugins
  can't call, and intake is the scan path the app and the command
  line share. Core, like OCR (M5's 2026-10-11 ruling).
- Separation could be a plugin's (a plugin can render pages and read
  barcodes with its own wasm build of a decoder), but it needs what
  0.4 doesn't give plugins: inserting pages (§14.3: "insert-pages …
  later, with manifest permissions"), a new page source, the scan
  path and `sheaf separate`. And it's the PDFlyer parity row, so it
  belongs with the binder tools.
- **What plugins get:** `PageSource::Separator` is an op's, so it
  reaches plugins and `sheaf edit` when inserts open to them; the
  bookmarks and pages a separation makes are ordinary ones, which
  `tax-form-bookmarks` and others read; Document > Separate is a
  command, which M15's batch and M16's agent can run.
