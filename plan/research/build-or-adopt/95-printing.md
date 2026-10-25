# Build or adopt 95: Printing (`sheaf-print`)

Area: Infrastructure that's built (confirmed) · Part of [build or adopt](README.md)

- **Where:** M7, done (`plan/complete/m07-viewing.md`;
  `crates/sheaf-print/src/lib.rs:5-9`). B28.
- **Today:** "CUPS through its command-line tools … This avoids linking
  libcups … No desktop portal: it's sandbox-only and offers no preview
  control" (`m7.md:23-30`). Windows uses GDI, with PDFium rasterizing.
- **Candidates:** [printers](https://crates.io/crates/printers) 2.3.0
  (2026-02-06; MIT; libcups on Unix, winspool on Windows; 120k
  downloads). sheaf rejected linking libcups on purpose.
- **Gap:** the Flatpak build is sandboxed. Inside it, `lp` must exist in
  the runtime or be bundled, and the app needs `--socket=cups` (Flatpak
  1.6 and later). Whether the freedesktop runtime ships `lp` is
  unverified ([flatpak-run](https://www.mankier.com/1/flatpak-run),
  [KDE's `--socket=cups`](https://phabricator.kde.org/R256:2415ebe0b7ed42a2eed468c9da10d3924dd9b1c1)).
- **Recommendation:** **build in-house (keep).** Add a Flatpak printing
  check to the packaging work: bundle the CUPS client tools with
  `--socket=cups`, or use the print portal only in the Flatpak. Medium.
