# M5 proposal 5: Which driver API on which system

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**Which driver API on which system.**

| System | Driver API | Covers | Not covered |
|---|---|---|---|
| Linux | SANE (`libsane.so.1`, the system's) | USB scanners with a SANE backend (fujitsu covers fi-series and ScanSnap iX1600, canon_dr, epsonds, avision, kodak), vendors' own backends, network scanners through sane-airscan (eSCL, WSD) and hplip | scanners with no backend |
| Windows | **TWAIN** (TWAINDSM, 64- and 32-bit), then **WIA 2.0** | TWAIN: every office document scanner's own driver (PaperStream IP, Canon's, Epson Scan 2, Kodak's, Brother's); WIA: scanners with a WIA driver, and network scanners and copiers through Windows' built-in WSD and eSCL drivers | ScanSnap (no driver API; proposal 11) |
| macOS | ask 4 | — | — |

- **No network scan protocol of sheaf's own.** eSCL looked like a
  common path for all three systems, but the scanners tax offices put
  on desks are USB-only (fi-7160 and fi-8170, Canon DR-C225, ScanSnap),
  and each system already reaches network scanners: sane-airscan on
  Linux, WIA's built-in drivers on Windows, AirScan through Image
  Capture on macOS. Mopria's eSCL specification is licensed for
  internal implementation only, with no redistribution. A direct
  eSCL client stays an option for later.
- **Tax software asks for TWAIN.** Drake Documents, FileCabinet CS,
  TaxDome and Scan123 all name TWAIN; PaperStream IP (fi-series) is a
  TWAIN and ISIS driver. WIA covers fewer office scanners but comes
  with Windows, and is how a network copier appears.
- Order on Windows: the device list shows both, marked by driver,
  with TWAIN first when one scanner has both.
