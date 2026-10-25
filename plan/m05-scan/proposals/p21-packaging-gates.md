# M5 proposal 21: Packaging and gates

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**Packaging and gates.**
- **Every bundle** gains `sheaf-scan-worker` beside `sheaf` and
  `sheaf-cli` (`scripts/package.sh`), found as PDFium is: beside the
  executable, then `target/` under test.
- **Linux:** nothing else. SANE is the system's, loaded at run time;
  a new CI gate keeps it so: `! ldd target/debug/sheaf-scan-worker |
  grep libsane`, beside the OCR libraries' gate.
- **Windows:** `sheaf-scan-worker-x86.exe` (if ask 3 keeps 32-bit
  TWAIN), built for `i686-pc-windows-msvc` (the worker alone: no
  GPUI, PDFium or Tesseract, so a short build), and `TWAINDSM.dll` in
  both bitnesses with its LGPL-2.1 text in `packaging/licenses/`.
  [`code-signing.md`](../../topics/code-signing.md)'s table of PE files grows by
  four.
- **macOS:** if ask 4 brings ImageCaptureCore, it's a system
  framework, which the "only system libraries" gate allows.
- **A crate gate:** `sheaf-scanner` stays devices only: no
  `sheaf-doc`, `sheaf-render`, `sheaf-ocr`, `lopdf`, PDFium or GPUI
  under it (the shape of `sheaf-image`'s gate).
- **Sandboxes, for when packaging comes** (not M5): there is no
  scanner portal (xdg-desktop-portal #218, open since 2018). Simple
  Scan's Flatpak bundles sane-backends, sane-airscan, hplip and avahi
  and asks for `--device=all` and `--share=network`; NAPS2's adds
  `--filesystem=host` to reach the host's backends. A Snap needs
  `raw-usb`, connected by hand.
