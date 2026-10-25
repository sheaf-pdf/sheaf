# M5 proposal 6: SANE: loaded at run time, sheaf's own bindings

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**SANE: loaded at run time, sheaf's own bindings.**
- `libsane.so.1` is `dlopen`ed by the worker (`libloading`), as PDFium
  and ONNX Runtime are by the app. Without it, sheaf starts and runs as
  ever, and the scanner dialog says "Scanning on Linux needs SANE
  (the libsane1 package)". `SHEAF_LIBSANE` names another library, for
  tests (slice 6).
- The bindings are sheaf's: the 15 functions and their structs, from
  SANE's public-domain `sane.h`. The one crate (`sane-scan` 0.1.2,
  2023) links libsane at build time and runs bindgen on the system's
  headers, and has no cancel from another thread.
- **Licenses:** SANE's backends are GPL-2.0+ with the SANE exception
  ("linking … does not by itself cause the resulting executable to be
  covered by the GPL"), and `sane.h` is public domain. sheaf neither
  links nor ships SANE: it loads the system's, so nothing of SANE is
  distributed with it.
- **What every frontend learned, built in:**
  - one thread per handle, nothing overlapping; `sane_cancel` from the
    watchdog's thread is allowed (the API calls it safe
    asynchronously);
  - `sane_cancel` after every batch and after any error status;
  - a feeder: `sane_start` per page until `SANE_STATUS_NO_DOCS`, which
    on the first page means "the feeder is empty" and later means
    done, and which some backends return from `sane_read`, not
    `sane_start`;
  - a flatbed: one page per Scan press (some flatbeds give the same
    page forever);
  - parameters that make no sense (Brother's `2500x0`) end the batch;
  - `lines = -1` reads to EOF; padded rows; 1-bit rows are 1 = black,
    first pixel in the high bit; 16-bit samples in native order;
    three-pass frames assembled;
  - options by their standard names (`resolution`, `mode`, `source`,
    `tl-x`…`br-y`, `page-width`/`page-height`), and the feeder and
    duplex by what backends actually call them: `source` values `ADF
    Front`/`ADF Back`/`ADF Duplex` (fujitsu, canon_dr, avision),
    `ADF`/`ADF Duplex` (airscan), `Automatic Document Feeder` (test),
    vendor strings with "Duplex" in them, or a separate `adf-mode`,
    `duplex` or `ScanMode` option (simple-scan keeps the same list);
  - `page-height` set after `source`, since the fujitsu backend only
    enables it for the feeder; descriptors read again whenever an
    option says so, or `sane_start` returns `SANE_STATUS_INVAL`.
- **Finding scanners:** `sane_get_devices` in a short-lived worker
  with its own deadline (it took 10.7 s here with nothing attached);
  the last device is opened by its name directly, without a list.
