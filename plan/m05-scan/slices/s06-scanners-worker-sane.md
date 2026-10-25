# M5 slice 6: Scanners: the worker and SANE

Status: deferred · Part of [M5: Scan & OCR](../README.md)

**Scanners: the worker and SANE** (proposals 4, 5, 6, 9 and 22).
- **Builds:** `sheaf-scanner` (model, protocol, client, `fake`, `sane`),
  `sheaf-scan-worker`, sheaf's fake libsane (tests), `sheaf scan
  --list-devices` and `--device` (intake from a scanner into a
  Session), `--separate`; `[scanner]` in the settings template; CI
  installs `libsane1`; the `ldd` and crate gates.
- **Checks:** the `fake` backend's scripts (proposal 22) on every
  runner; the fake libsane's cases and SANE's test backend on Linux;
  a hang is killed by the watchdog with pages kept; a crash keeps
  pages and says the exit; `--list-devices` with SANE missing says
  why and exits 1; by hand, an fi-6130 through SANE's fujitsu
  backend on a Linux laptop at the office (ask 12).
- **Scan-speed gate** (Greg, 2026-10-23): the intake keeps pace with a
  fi-8170, 140 images a minute duplex (2.3 a second) at 300 dpi, in
  color and in black and white, fed by the `fake` backend through the
  worker's protocol. Cleanup and encoding keep up with the feeder
  (pages cleaned and encoded at least as fast as they arrive, measured
  over 200 images after the first 20); OCR may fall behind and finish
  after the feeder stops, with memory bounded while it does (the
  waiting pages held encoded, not as pixels). The gate fixes the
  worker threads to an office PC's count (4 cores; the notes say the
  CPU it ran on), is measured on every run and asserted with
  `SHEAF_GATE_SCAN_SPEED`. Slice 1 measured cleanup at about 0.8 s a
  300 dpi Letter page on one thread under Scanner (3.2 s under Photo),
  so meeting 2.3 a second on 4 cores needs pages cleaned in parallel
  (a few in flight, landed in order), or a faster cleanup.
