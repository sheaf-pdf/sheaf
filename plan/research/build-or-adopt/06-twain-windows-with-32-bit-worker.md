# Build or adopt 6: TWAIN on Windows, with a 32-bit worker

Area: M5: scanner intake (the `m5-intake` branch) · Part of [build or adopt](README.md)

- **Need:** drive TWAIN data sources, 64- and 32-bit. The office's
  Fujitsu fi-6130s use the 32-bit FUJITSU TWAIN32 driver.
- **Where:** M5 (`m5-intake.md` proposal 7, asks 3 and 12).
- **Today's plan:** build. "Bindings: sheaf's own, from `twain.h` (the
  `twain` crate is a single 2021 release that ships DLLs inside it).
  RSWebTWAIN … is the reference" (`m5-intake.md:675-678`).
- **Candidates:**
  - [`twain`](https://crates.io/crates/twain) 0.1.0 (2021-07-24, the only
    release; MIT; about 2k downloads; 791 KB because it carries DLLs;
    FFI). No `twain-sys` exists.
  - [RSWebTWAIN](https://github.com/seancampbell3161/RSWebTWAIN) (MIT and
    Apache-2.0 files; 0 stars; pushed 2026-10-16): a Tauri app, "alpha",
    not a crate. Its typestate TWAIN state machine and 32-bit JSON-lines
    sidecar are the plan's reference.
  - Other Rust TWAIN FFI, such as `jtubbenhauer/twain-rs` (MIT, one day
    of commits in 2024), is app-internal or abandoned.
  - The TWAIN DSM (`TWAINDSM.dll`, LGPL-2.1): the plan ships it
    unmodified and loads it at run time (`m5-intake.md:660-662`). That's
    LGPL-compatible, but a cost: the license text, a source offer or
    link, and four more PE files to sign (the plan lists the first and
    the last). See D17.
- **Fit:** no crate covers 32- plus 64-bit, the hidden window and
  message loop, or memory transfer.
- **Recommendation:** **build in-house**, as planned. High.
