# M5 proposal 11: Scanners sheaf can't drive hand it files

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**Scanners sheaf can't drive hand it files.**
- **ScanSnap** has no TWAIN, ISIS or WIA driver (PFU's FAQ); on
  Linux, SANE's fujitsu backend drives the iX1600 over USB. On
  Windows and macOS, a ScanSnap Home profile can **Send to** any
  application, which it starts with the scanned file. With sheaf as
  that application, each scan opens in the running sheaf
  (single instance hands it over), where Document > Separate does the
  rest. The docs gain a page on setting that profile up.
- **Copiers' scan-to-folder** and scan-to-email files are opened, or
  inserted, as any PDF; Separate works on them the same way.
- **Later, not M5:** ScanSnap's SDK (Windows and macOS, registration
  required) and watched folders (M15).
