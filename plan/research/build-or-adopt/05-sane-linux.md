# Build or adopt 5: SANE on Linux

Area: M5: scanner intake (the `m5-intake` branch) · Part of [build or adopt](README.md)

- **Need:** list SANE scanners, read their options and scan from a
  worker process, with sheaf starting even when libsane is absent.
- **Where:** M5 scanner intake (`ROADMAP.md:130`); `m5-intake.md`
  proposal 6.
- **Today's plan:** build. "`libsane.so.1` is `dlopen`ed by the worker
  (`libloading`)… The bindings are sheaf's: the 15 functions and their
  structs, from SANE's public-domain `sane.h`. The one crate (`sane-scan`
  0.1.2, 2023) links libsane at build time" (`m5-intake.md:615-624`).
- **Candidates:**
  - [`sane-scan`](https://crates.io/crates/sane-scan) 0.1.2 (2023-03-10;
    three releases; MIT on crates.io, though GitHub shows no license
    file; about 4.3k downloads, 22 in the last 90 days; last push
    2024-05). Links libsane at build time and runs bindgen on the system
    headers. Not for wasm.
  - [`libsane-sys`](https://crates.io/crates/libsane-sys) **doesn't
    exist** (crates.io returns 404), and nor does `sane-sys`. The crate
    named [`sane`](https://crates.io/crates/sane) (0.2.1, 2019) is a
    serialization format.
  - About a dozen apps on GitHub carry their own SANE FFI (a code search
    for `sane_get_devices` in Rust); none is a maintained library:
    [libsane-rs](https://github.com/ColinTimBarndt/libsane-rs)
    (Apache-2.0, two days of commits in 2024-06, unpublished) and
    [ruscan](https://github.com/iruizr7/ruscan) (MIT, a GTK4 app, 0.1.0,
    1 star).
  - [`libloading`](https://crates.io/crates/libloading) 0.9.0
    (2025-11-05; ISC; 590M downloads), already in the lockfile.
- **Fit:** sheaf needs run-time loading, cancel from a watchdog thread,
  and its own handling of backend quirks; no crate offers these.
  - Flatpak is a cost the plan defers: it would bundle sane-backends, as
    Simple Scan's does (`m5-intake.md:1090-1095`). That's GPL-2.0+ with
    the SANE exception. Loading it at run time keeps sheaf's license
    intact, but the bundle must offer the backends' source; Flathub's
    public manifests normally cover that (unverified for sheaf). See D17.
- **Recommendation:** **build in-house**, as planned. High.
