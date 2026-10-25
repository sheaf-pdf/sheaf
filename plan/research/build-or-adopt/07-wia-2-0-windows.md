# Build or adopt 7: WIA 2.0 on Windows

Area: M5: scanner intake (the `m5-intake` branch) · Part of [build or adopt](README.md)

- **Need:** feeder and duplex scanning, and network copiers through
  Windows' WSD and eSCL drivers.
- **Where:** M5 (`m5-intake.md` proposal 8).
- **Today's plan:** adopt `windows`: "`Win32::Devices::ImageAcquisition`:
  `IWiaDevMgr2`, `IWiaItem2`, `IWiaTransfer`, `IWiaTransferCallback`"
  (`m5-intake.md:680-683`).
- **Candidates:** [`windows`](https://crates.io/crates/windows) 0.62.2
  (2025-10-06; MIT OR Apache-2.0; Microsoft; 375M downloads). Still the
  latest: windows-rs's 2026 releases published only windows-bindgen
  0.66, windows-metadata 0.60 and windows-rdl
  ([releases](https://github.com/microsoft/windows-rs/releases)). Feature
  `Win32_Devices_ImageAcquisition` has `IWiaDevMgr2`, `IWiaItem2`,
  `IWiaTransfer`, `IWiaTransferCallback`, `WIA_IPS_DOCUMENT_HANDLING_SELECT`,
  `WIA_IPS_PAGES` and `WIA_ERROR_PAPER_EMPTY`
  ([docs](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Devices/ImageAcquisition/index.html)).
- **Fit:** complete for the calls planned. The undocumented `0x210001`
  status is a raw HRESULT sheaf matches itself.
- **Recommendation:** **adopt.** Put the worker on GPUI's version (0.62)
  so the 64-bit build adds nothing. High.
