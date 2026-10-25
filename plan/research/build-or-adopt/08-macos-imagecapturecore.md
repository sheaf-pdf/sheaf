# Build or adopt 8: macOS ImageCaptureCore (not in M5)

Area: M5: scanner intake (the `m5-intake` branch) · Part of [build or adopt](README.md)

- **Need:** scanning on macOS, later.
- **Where:** `m5-intake.md` ask 4 (`:1217-1229`), a Pending entry.
- **Today's plan:** "`objc2-image-capture-core` (0.3.2, which has
  `ICScannerDevice` and the feeder unit)".
- **Candidates:** [`objc2-image-capture-core`](https://crates.io/crates/objc2-image-capture-core)
  0.3.2 (2025-10-04; Zlib OR Apache-2.0 OR MIT; madsmtm's objc2, pushed
  2026-10-17). Still current: every objc2 framework crate is on 0.3.2.
  Features `ICScannerDevice`, `ICScannerFunctionalUnits` and
  `ICScannerBandData`. FFI to a system framework, which passes the
  plan's "only system libraries" gate.
- **Recommendation:** **adopt**, when it's scheduled. Medium (no Mac
  scanner to test with).
