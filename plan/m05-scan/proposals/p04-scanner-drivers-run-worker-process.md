# M5 proposal 4: Scanner drivers run in a worker process

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**Scanner drivers run in a worker process.** `sheaf-scan-worker`, a
small binary of `sheaf-scanner` alone (no GPUI, PDFium or OCR),
shipped beside `sheaf` and `sheaf-cli`. The app and the command line
start it when they need a scanner and talk to it over its standard
input and output.
- **Why:** a scanner driver is someone else's native code in sheaf's
  address space, beside documents with unsaved changes. libinsane
  runs SANE in its own process because some backends (Brother, old HP)
  corrupt memory and crash; NAPS2 runs SANE and Apple's in workers "for
  added stability"; `scanimage` hung on this machine after an injected
  jam. A worker that crashes or hangs is killed and restarted, and the
  pages it already sent are kept. TWAIN needs it anyway: a 32-bit
  driver loads only into a 32-bit process, and TWAIN wants a window
  and a message loop, which the worker owns.
- **The protocol:** requests as JSON lines in (`list`, `caps`, `scan`,
  `cancel`, `quit`), events as JSON lines out (`device`, `caps`,
  `sheet`, `page`, `status`, `error`, `done`), each `page` followed by
  its bytes (raw pixels with width, height, format and resolution, or
  an encoded image as the driver gave it). Versioned by a `hello`
  line; the worker and the app ship together, so a mismatch is an
  error, not a negotiation.
- **Watchdog:** a scan that sends nothing for 60 seconds (a flatbed's
  lamp warm-up included; `[scanner] timeout` changes it) is ended: the
  worker is killed, the reader is told "the scanner stopped
  answering", and the pages so far stay. Cancel is a message; a
  worker that hasn't stopped 10 seconds later is killed.
- **Windows:** `sheaf-scan-worker.exe` (64-bit) and
  `sheaf-scan-worker-x86.exe` (32-bit, for 32-bit TWAIN drivers; ask
  3). Each lists the TWAIN sources of its own bitness; the client
  merges the lists and uses the worker the source needs.
- **One driver per worker,** started for the dialog and kept while it's
  open (opening a SANE device can take seconds); a `list` has its own
  short-lived worker, so a slow probe never holds a scan up.
- Alternative: in process, on a thread of their own (simple-scan,
  gscan2pdf). Simpler, but a driver's crash takes sheaf with it, and
  32-bit TWAIN can't load at all.
