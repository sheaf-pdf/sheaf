# M5 research: Scanners

Part of [M5: Scan & OCR](../README.md).


- **What tax offices scan with** (what tax software names and
  certifies; no survey of offices was found). Desk ADF scanners, mostly
  USB: Fujitsu/Ricoh fi-7160 and fi-8170 (PaperStream IP, a TWAIN and ISIS
  driver), Canon imageFORMULA DR (TWAIN and ISIS, some WIA), Epson DS
  (TWAIN, WIA, ISIS, ICA, SANE), Brother ADS (TWAIN, WIA, ISIS, SANE),
  Kodak S2000 (TWAIN, ISIS, WIA), and ScanSnaps. Tax software asks for
  TWAIN: Drake Documents, Thomson Reuters FileCabinet CS, TaxDome (WIA or
  TWAIN; it lists ScanSnap iX100–iX1600 as having neither), Scan123 (which
  calls ScanSnap "completely incompatible").
- **ScanSnap** "does not support TWAIN drivers or ISIS drivers" (PFU's
  FAQ), and no WIA driver exists. ScanSnap Home profiles can **Send to**
  any application by its executable. A ScanSnap SDK exists for Windows
  and macOS, with registration. On Linux, SANE's fujitsu backend rates
  the iX1600 "good" over USB.
- **SANE.** A small C API (`sane_init` … `sane_exit`), no thread-safety
  promise (calls on one handle mustn't overlap; `sane_cancel` is safe
  asynchronously). Batch scanning isn't in the standard: frontends call
  `sane_start` per page until `NO_DOCS`, which some backends return from
  `sane_read`. Option names are standard; the feeder's and duplex's
  values aren't (`ADF Duplex`, `ADF Front`, `Automatic Document Feeder`,
  separate `adf-mode` or `duplex` options). License: backends GPL-2.0+
  with the SANE exception, `sane.h` public domain. The one Rust crate
  (`sane-scan` 0.1.2, 2023) links at build time; ruscan (MIT) `dlopen`s.
  NAPS2 runs SANE in a worker; libinsane runs it in its own process
  because some backends corrupt memory and crash.
- **SANE's `test` backend, here.** `libsane-test.so` comes with
  libsane1 but is commented out of `/etc/sane.d/dll.conf`; a private
  `SANE_CONFIG_DIR` whose `dll.conf` says `test` turns it on without
  root. It simulates a feeder (empty after 10 sheets), gray and color,
  1/8/16-bit, unknown height, short reads, and injected statuses (jam,
  cover open, no documents, I/O error). No duplex; its area stops at 200
  mm. `scanimage -L` with this machine's own configuration took 10.7 s
  with no scanner attached.
- **eSCL** (AirScan, Mopria) is HTTP and XML, found by mDNS
  (`_uscan._tcp`). Network document scanners and copiers speak it; the
  USB desk scanners above don't (an fi-series has no eSCL; HP's USB
  ScanJets speak it only over IPP-over-USB). Each system reaches eSCL
  already: sane-airscan on Linux; Windows' built-in drivers, WSD's as
  WIA devices and, on Windows 11, eSCL's (a 24H2 bug broke discovery of
  USB eSCL devices; that every eSCL copier appears under WIA is
  unverified); macOS's AirScan through Image Capture. Mopria's specification is free to download under a
  license for internal implementation only, with no redistribution.
  sane-airscan's source lists the quirks a client must handle (503
  retries, Brother's false "ADF empty", Xerox's jam that means empty).
- **WIA 2.0** is in the `windows` crate (`IWiaDevMgr2`, `IWiaItem2`,
  `IWiaTransfer`); feeder and duplex by `WIA_IPS_DOCUMENT_HANDLING_SELECT`,
  `WIA_IPS_PAGES = 0` for the whole feeder; errors by code (paper empty,
  jam, multi-feed, cover open, warming up). WIA 1.0 and WinRT's scanner
  API scan only to files. No virtual WIA device installs without a
  person.
- **TWAIN.** Windows ships only the old 32-bit `TWAIN_32.DLL`;
  `TWAINDSM.dll` (LGPL-2.1) comes with drivers. A data source loads only
  into a process of its own bitness; NAPS2 runs TWAIN in a 32-bit worker
  because drivers are "generally 32-bit only", though PaperStream IP,
  Epson Scan 2 and newer Canon drivers have 64-bit TWAIN. TWAIN wants a
  window handle and a message loop. `twain.h` defines patch-code and
  barcode detection (`ICAP_PATCHCODEDETECTIONENABLED`, `TWEI_PATCHCODE`,
  `TWEI_BARCODETEXT`); which drivers fill them in is unverified. The TWAIN
  Working Group's sample data source is a virtual scanner (Modified BSD,
  MSI installers); a silent install on GitHub's runners is unverified.
  Rust: the `twain` crate is one 2021 release; RSWebTWAIN (Apache-2.0,
  alpha) has the 64-bit app plus 32-bit sidecar shape.
- **macOS.** ImageCaptureCore (`ICScannerDevice`, a feeder unit with
  duplex) is wrapped by `objc2-image-capture-core` 0.3.2; it reaches ICA
  drivers and AirScan devices in one API. TWAIN on macOS is gone (the
  TWAIN Bridge left in 10.9). Apple's VirtualScanner sample runs by hand,
  not in CI (unverified). Direct network scanning from an app is behind
  the Local Network privacy prompt since macOS 15.
- **Sandboxes.** No scanner portal (xdg-desktop-portal #218, open since
  2018). Flathub's Simple Scan bundles SANE and asks for every device and
  the network.
- **NAPS2** (GPL-2.0+, SDK LGPL-2.1+): WIA, TWAIN (32-bit worker), SANE
  and Apple's API in workers, and eSCL; its issues are the best list of
  driver quirks (a jam that persists after clearing; Kodak's backend
  defaulting to duplex; WIA's `0x210001`). Its CI runs no scanner; its
  scanner tests are a manual project ("put the test page in your
  scanner").
