# M5 proposal 7: TWAIN on Windows

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**TWAIN on Windows.**
- The worker loads `TWAINDSM.dll` (the system's, installed by drivers;
  else sheaf's bundled copy of the TWAIN Working Group's, LGPL-2.1,
  unmodified, loaded at run time), opens it with a hidden window it
  owns, and pumps that window's messages through `MSG_PROCESSEVENT`.
- Scans without the driver's window by default (`ShowUI = false`),
  with sheaf's settings: `ICAP_XRESOLUTION`, `ICAP_PIXELTYPE`,
  `CAP_FEEDERENABLED`, `CAP_DUPLEXENABLED`, `ICAP_SUPPORTEDSIZES` or a
  frame, `CAP_XFERCOUNT = -1`; native or memory transfer.
- **"Scanner settings…"** opens the driver's own window
  (`CAP_ENABLEDSUIONLY`, where the driver has it), for what sheaf
  doesn't set: PaperStream's own image processing, a dropout color,
  a driver's double-feed sensitivity. Those then apply to sheaf's scans
  until changed.
- Errors: `TWCC_PAPERJAM`, `TWCC_PAPERDOUBLEFEED`, `TWCC_NOMEDIA`,
  `CAP_FEEDERLOADED` before starting, cover open.
- Bindings: sheaf's own, from `twain.h` (the `twain` crate is a single
  2021 release that ships DLLs inside it). RSWebTWAIN (Apache-2.0,
  alpha: a 64-bit app with a 32-bit JSON-lines sidecar and a typestate
  TWAIN state machine) is the reference.
