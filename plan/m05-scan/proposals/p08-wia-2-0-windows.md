# M5 proposal 8: WIA 2.0 on Windows

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**WIA 2.0 on Windows.**
- Through the `windows` crate (`Win32::Devices::ImageAcquisition`:
  `IWiaDevMgr2`, `IWiaItem2`, `IWiaTransfer`,
  `IWiaTransferCallback`), on the worker's own STA thread.
- The feeder is the root's `Feeder` child; duplex is
  `WIA_IPS_DOCUMENT_HANDLING_SELECT = FEEDER | DUPLEX`;
  `WIA_IPS_PAGES = 0` scans until the feeder is empty, a sheet being
  two pages in duplex.
- Errors by code: `WIA_ERROR_PAPER_EMPTY` (at the start, "the feeder
  is empty"; later, done), `PAPER_JAM`, `MULTI_FEED`, `COVER_OPEN`,
  `WARMING_UP` (wait and retry), and the undocumented `0x210001` (no
  more pages), as NAPS2 learned.
- WIA 1.0 and the WinRT scanner API aren't used: both scan only to
  files.
