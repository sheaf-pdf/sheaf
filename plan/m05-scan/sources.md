# M5: Sources

Part of [M5: Scan & OCR](README.md).

## Research for this plan

Research for this plan, all on 2026-10-23:
- sheaf's code at `3e1b9f0`: slice 19's `Session`, memory sources,
  exports, `sheaf edit` and the Op vocabulary, the scan path and the app's
  Create from Scans, Find Blank Pages' planner and review;
- this machine's SANE (sane-backends 1.2.1 libraries, libsane1 1.4.0,
  sane-airscan 0.99.36, hplip), with the `test` backend turned on in a
  private config directory and driven by `scanimage`;
- a benchmark of barcode reading in Rust on synthetic 300 dpi pages
  (a scratch crate outside the repo);
- the 709 scanned PDFs in `../pdf-corpus/scans/`;
- the prior-art notes (`docs/prior-art/pdflyer.md` §2.7, §4–§6,
  `acrobat.md` §2.11, `README.md`);
- outside sources, listed in [Sources](sources.md).


Checked 2026-10-23.

- **This machine and the code:** sheaf at `3e1b9f0`; sane-backends'
  `scanimage` 1.2.1 with libsane1 1.4.0, sane-airscan 0.99.36 and
  hplip 3.24.4; the `test` backend through a private `SANE_CONFIG_DIR`;
  `../pdf-corpus/scans/` (709 files, `manifest.tsv`).
- **SANE:**
  [the API](https://sane-project.gitlab.io/standard/api.html),
  [image data](https://sane-project.gitlab.io/standard/environment.html),
  [`saneopts.h`](https://gitlab.com/sane-project/backends/-/blob/master/include/sane/saneopts.h),
  [LICENSE](https://gitlab.com/sane-project/backends/-/blob/master/LICENSE),
  [`dll.c`'s exception](https://gitlab.com/sane-project/backends/-/blob/master/backend/dll.c),
  [`scanimage.c`](https://gitlab.com/sane-project/backends/-/blob/master/frontend/scanimage.c),
  [fujitsu.desc](https://gitlab.com/sane-project/backends/-/blob/master/doc/descriptions/fujitsu.desc),
  sane-devel on [threads](https://lists.alioth.debian.org/pipermail/sane-devel/2002-December/045285.html),
  [ADF](https://lists.alioth.debian.org/pipermail/sane-devel/2002-December/045351.html)
  and [fujitsu's page-height](https://lists.alioth.debian.org/pipermail/sane-devel/2011-June/028762.html);
  [`sane-scan`](https://crates.io/crates/sane-scan),
  [ruscan](https://github.com/iruizr7/ruscan);
  simple-scan's [`scanner.vala`](https://gitlab.gnome.org/GNOME/simple-scan/-/blob/master/src/scanner.vala),
  gscan2pdf's [`Image_Sane.pm`](https://github.com/carygravel/gscan2pdf/blob/master/lib/Gscan2pdf/Frontend/Image_Sane.pm),
  libinsane's [`workarounds.h`](https://gitlab.gnome.org/World/OpenPaperwork/libinsane/-/blob/master/subprojects/libinsane/include/libinsane/workarounds.h)
  and [scan APIs](https://gitlab.gnome.org/World/OpenPaperwork/libinsane/-/blob/master/subprojects/libinsane/doc/scan_apis.dox).
- **eSCL and network scanning:**
  [Debian wiki](https://wiki.debian.org/eSCL),
  sane-airscan's [README](https://github.com/alexpevzner/sane-airscan),
  [`airscan-escl.c`](https://github.com/alexpevzner/sane-airscan/blob/master/airscan-escl.c),
  [`airscan-mdns.c`](https://github.com/alexpevzner/sane-airscan/blob/master/airscan-mdns.c),
  [`airscan.h`](https://github.com/alexpevzner/sane-airscan/blob/master/airscan.h),
  [LICENSE](https://github.com/alexpevzner/sane-airscan/blob/master/LICENSE)
  and [sane-airscan(5)](https://github.com/alexpevzner/sane-airscan/blob/master/sane-airscan.5.md);
  [Mopria's specification download](https://mopria.org/spec-download);
  [a PWG post on the job flow](https://pwg.org/archives/ipp/2019/020120.html);
  [WS-Scan](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wsd-scan-service-operation-elements);
  [Windows 11's eSCL issue](https://www.theregister.com/2024/11/26/microsoft_escl_issue/);
  [OpenPrinting on eSCL replacing SANE](https://lists.linuxfoundation.org/pipermail/printing-architecture/2021/003989.html);
  [ipp-usb](https://github.com/OpenPrinting/ipp-usb);
  test servers: [go-mfp](https://github.com/OpenPrinting/go-mfp),
  [AirSane](https://github.com/SimulPiscator/AirSane);
  crates: [`escl`](https://crates.io/crates/escl),
  [`mdns-sd`](https://crates.io/crates/mdns-sd).
- **Windows:**
  [WIA in the `windows` crate](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Devices/ImageAcquisition/index.html),
  [feeder item trees](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/implementing-feeder-scanner-item-trees),
  [duplex](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/simple-duplex-capable-document-feeder),
  [`WIA_IPS_PAGES`](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ips-pages),
  [WIA error codes](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/wia/-wia-error-codes),
  [WinRT's ImageScanner](https://learn.microsoft.com/en-us/uwp/api/windows.devices.scanners.imagescanner.scanfilestofolderasync),
  [sample WIA drivers](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/sample-wia-drivers),
  [fakescan](https://github.com/b1tninja/fakescan);
  TWAIN: [twain-dsm](https://github.com/twain/twain-dsm) and its
  [license](https://github.com/twain/twain-dsm/blob/master/TWAIN_DSM/license.txt),
  [the sample data source](https://github.com/twain/twain-samples/blob/master/TWAIN-Samples/Twain_DS_sample01/README.txt)
  and its [v2.5.0 installers](https://github.com/twain/twain-samples/releases/tag/v2.5.0),
  twain.org forum threads on [TWAINDSM vs TWAIN_32](https://twain.org/forums/topic/does-twaindsm-dll-replace-twain_32-dll/),
  [twain_32 and twain_64](https://www.twain.org/forums/topic/question-get-datasource-file-from-twain_64-folder/)
  and [the message loop](https://twain.org/forums/topic/twain-application-windows-c-callbacks-win-message-loop/),
  [TWAIN Direct](https://www.twaindirect.org/specification/),
  [the `twain` crate](https://crates.io/crates/twain),
  [RSWebTWAIN](https://github.com/seancampbell3161/RSWebTWAIN).
- **Scanners and drivers:**
  [PaperStream IP TWAIN x64](https://www.pfu.ricoh.com/global/scanners/fi/dl/setup/psip-twain64-3300.html),
  [fi-series drivers](https://www.pfu.ricoh.com/global/scanners/fi/dl/win-10-fi-8x70.html),
  [PFU's multi-OS drivers](https://www.pfu.ricoh.com/fi/software/multios-driver/),
  [Epson Scan 2's bitness](https://www.epson.eu/en_EU/faq/KA-01076/contents),
  Canon [DR-M260](https://asia.canon/en/support/0100967919) and
  [DR-C240](https://canon.jp/support/software/os/select/dr/DR-C240_Driver_V141171218001SP4_Windows),
  [Canon DR-S250N](https://www.canon-europe.com/business/products/scanners/document-scanners/imageformula-dr-s250n/specifications),
  [Brother ADS-4700W](https://brother-usa.com/-/media/brother/product-catalog-media/documents/ads-4700w-2-page-brochure.pdf),
  [Kodak S2000](https://alarisworld.com/solutions/document-scanners/desktop/s2080w-scanner);
  ScanSnap: [no TWAIN or ISIS](https://scansnap-faq.pfu.ricoh.com/hc/en-us/articles/27610858674713-Does-the-ScanSnap-Series-support-TWAIN-or-ISIS),
  [no WIA](https://www.technibble.com/forums/threads/wia-driver-for-fujitsu-scansnap-ix1600.87977),
  [its SDK](https://www.pfu.ricoh.com/scansnap/support/sdk/info-sdk.html),
  ScanSnap Home's [applications](https://www.pfu.ricoh.com/imaging/downloads/manual/ss_webhelp/en/help/webhelp/topic/ope_appli_mgr.html)
  and [profiles' Send to](https://www.pfu.ricoh.com/imaging/downloads/manual/ss_webhelp/en/help/webhelp/topic/ope_profile_make.html),
  [ScanSnap Home's split](https://scansnap-faq.pfu.ricoh.com/hc/en-us/articles/21000580939801-How-can-I-scan-multiple-documents-and-save-them-as-separated-PDF-files-in-a-batch);
  tax software: [FileCabinet CS](https://www.thomsonreuters.com/en-us/help/filecabinet-cs/send-to-fcs/determining-if-a-scanner-is-twain-compliant.html),
  [Drake Documents](https://kb.drakesoftware.com/KB/Drake-Documents/11416.htm) (search excerpt only),
  [TaxDome](https://help.taxdome.com/article/563-scanners-troubleshooting),
  [Scan123](https://knowledge.scan123.com/recommended-scanners).
- **macOS:**
  [`ICScannerDevice`](https://developer.apple.com/documentation/imagecapturecore/icscannerdevice),
  [the feeder unit](https://developer.apple.com/documentation/imagecapturecore/icscannerfunctionalunitdocumentfeeder),
  [`objc2-image-capture-core`](https://crates.io/crates/objc2-image-capture-core),
  [ScannerBrowser](https://developer.apple.com/library/archive/samplecode/ScannerBrowser/Listings/AppController_m.html),
  [a forum thread on finding no USB scanners](https://developer.apple.com/forums/thread/771629),
  [ICA bypassed for AirScan](https://support.lexmark.com/content/support/guides/en/v56544321/other-troubleshooting/software-and-solutions/image-capture-application-ica-bypasses-custom-driv.html),
  [TN3179, Local Network privacy](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy),
  [TWAIN on macOS](https://support.alarisworld.com/ps50-ps80-driver-for-mac),
  [VirtualScanner](https://developer.apple.com/library/archive/samplecode/VirtualScanner/Introduction/Intro.html).
- **Sandboxes:**
  [xdg-desktop-portal #218](https://github.com/flatpak/xdg-desktop-portal/issues/218),
  [Simple Scan's Flatpak](https://github.com/flathub/org.gnome.SimpleScan/blob/master/org.gnome.SimpleScan.json),
  [NAPS2's Flatpak](https://github.com/cyanfish/naps2/blob/master/NAPS2.Setup/config/linux/com.naps2.Naps2.yml),
  [Snap's simple-scan](https://forum.snapcraft.io/t/auto-connect-request-for-simple-scan/36173).
- **NAPS2:** [repository](https://github.com/cyanfish/naps2),
  [SDK README](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/README.md),
  [`SaneScanDriver.cs`](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Sane/SaneScanDriver.cs),
  [`TwainScanDriver.cs`](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Twain/TwainScanDriver.cs),
  [`WiaScanDriver.cs`](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Wia/WiaScanDriver.cs),
  [`ScanBridgeFactory.cs`](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/ScanBridgeFactory.cs),
  [`BarcodeDetector.cs`](https://github.com/cyanfish/naps2/blob/HEAD/NAPS2.Sdk/Scan/BarcodeDetector.cs),
  [`SaveSeparatorHelper.cs`](https://github.com/cyanfish/naps2/blob/HEAD/NAPS2.Lib/ImportExport/SaveSeparatorHelper.cs),
  [batch scan](https://naps2.com/doc/batch-scan),
  [troubleshooting](https://www.naps2.com/doc/troubleshooting),
  issues [#449](https://github.com/cyanfish/naps2/issues/449),
  [#581](https://github.com/cyanfish/naps2/issues/581),
  [#904](https://github.com/cyanfish/naps2/issues/904),
  [#916](https://github.com/cyanfish/naps2/issues/916),
  [its CI](https://github.com/cyanfish/naps2/blob/master/.github/workflows/dotnet.yml).
- **Separators, tax software:** CCH Scan Workstation help:
  [Using target sheets](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/Working/UsingTargetSheets.htm),
  [sheets](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/OP_and_SaaS_SharedTopics/sheets.htm),
  [separator sheets](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/OP_and_SaaS_SharedTopics/separator_sheet.htm),
  [Insert Target Sheets](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/PDFlyer/Insert_Target_Sheets.htm),
  [Funnel's inserts](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/ScanFunnel/SF-Insert-Cover-Pages-Target-Sheets.htm)
  and [tips](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/ScanFunnel/SF-Target-Sheet-Tips.htm),
  [the 2011 Scan user guide](https://manualzilla.com/doc/5813161/--support),
  [Scan 17.0's notes](https://support.cch.com/updates/Scan/pdf/ReleaseNotesScan17.0.pdf);
  SurePrep's [target sheets](https://www.thomsonreuters.com/en-us/help/sureprep/scanners/target-sheets.html),
  [their PDF](https://www.thomsonreuters.com/content/dam/helpandsupp/en-us/Topics/sureprep-family/files/Target-Sheets/all-target-sheets.pdf)
  and [scanning for SPbinder](https://www.thomsonreuters.com/en-us/help/sureprep/scanners/scanning-documents-for-spbinder.html);
  GoFileRoom ScanFlow's [cover sheets](https://www.thomsonreuters.com/en-us/help/gofileroom/scanflow/use/generate-scanflow-barcode-cover-sheets)
  and [scanning](https://www.thomsonreuters.com/en-us/help/gofileroom/scanflow/use/scan-documents-with-scanflow).
- **Separators, capture software:** Kofax Capture's
  [Separator Sheet Utility](https://docshield.tungstenautomation.com/KC/en_US/11.1.0-40hy9nfk91/help/main/KC_separatorsheetutility/c_sepsheetutil.html)
  and [separation profiles](https://docshield.tungstenautomation.com/KC/en_US/11.1.0-40hy9nfk91/help/main/KC_administration/c_customsepformidprofilewin.html),
  [TotalAgility](https://docshield.tungstenautomation.com/KTA/en_US/7.8.0-dpm5ap0jk8/help/TA/All_Shared/Capture/t_profilesepforadocument.html),
  [ReadSoft Scan Client](https://docshield.tungstenautomation.com/RSXBSC/en_US/5.10/help/XBSC/Content/procedures/setup/Using_barcodes_for_document_separation.htm),
  [Kofax Express](https://docshield.tungstenautomation.com/express/en_us/3.3.0-8vueggfgfv/help/express_help/Express_help/c_barcodeindexing.html),
  [Power PDF's split](https://docshield.tungstenautomation.com/PowerPDF/en_US/5.0.0-3uoz7ssq2b/help/PowerPDF_help/c_splitdocument.html),
  Kodak Capture Pro's [patch codes](https://resources.kodakalaris.com/support/Capture%20Pro%20Help/Content/About_patch_codes.htm)
  and [bar code zones](https://resources.kodakalaris.com/support/Capture%20Pro%20Help/Content/Separating_documents_with_bar_code_zones.htm),
  [PaperStream Capture](https://www.pfu.ricoh.com/global/scanners/fi/paperstreamcapture/)
  and [its change history](https://www.pfu.ricoh.com/global/scanners/fi/dl/update/history/psc-301-change.pdf),
  [Epson Document Capture Pro](https://files.support.epson.com/htmldocs/wfds50k/wfds50kug/scan2_2.htm),
  ABBYY [FineReader Server](https://help.abbyy.com/en-us/finereaderserver/14/help/idd_dialogseparationoptions/)
  and [FlexiCapture](https://help.abbyy.com/en-us/flexicapture/12/scanning_station/batch_type_separation/),
  [DocuWare](https://support.docuware.com/en-US/knowledgebase/article/KBA-37139),
  [capturebites on sheet design](https://www.capturebites.com/metaserver/help/separator-sheet-design).
- **Separators, editors and open source:**
  [Evermap AutoSplit](https://evermap.com/AutoSplit-Split.asp),
  [Foxit's split](https://resource-cms.foxit.com/foxit-pdf-editor-for-windows/split-pdfs-multiple-files),
  [PDF-XChange's split](https://help.pdf-xchange.com/pdfxt8/split-document_ed.html),
  Paperless-ngx's [barcode settings](https://docs.paperless-ngx.com/configuration/#barcodes),
  [`barcodes.py`](https://github.com/paperless-ngx/paperless-ngx/blob/dev/src/documents/barcodes.py)
  and [v3 migration notes](https://github.com/paperless-ngx/paperless-ngx/blob/dev/docs/migration-v3.md),
  [Stirling-PDF's auto split](https://github.com/Stirling-Tools/Stirling-PDF/blob/main/app/core/src/main/java/stirling/software/SPDF/controller/api/misc/AutoSplitPdfController.java),
  [scanprep](https://github.com/baltpeter/scanprep),
  [pss](https://github.com/kduma-OSS/CLI-pdf-scan-splitter),
  [ecoDMS's separator sheet](https://www.ecodms.de/en/download/ecodms-archive/separator-sheet).
- **Patch codes:** [Alliance's patch-T sheet](https://www.alliancegroup.co.uk/downloads/patch_t_us-letter.pdf)
  and [notes](https://www.alliancegroup.co.uk/patch-codes.htm),
  [Dynamsoft's patch code patterns](https://www.dynamsoft.com/barcode-reader/barcode-types/patch-code/)
  (a vendor page), [zxing-cpp's formats](https://github.com/zxing-cpp/zxing-cpp).
- **Barcode libraries:** [rxing](https://crates.io/crates/rxing)
  ([repository](https://github.com/rxing-core/rxing), its
  [Cargo.toml's `image` pin](https://docs.rs/crate/rxing/0.9.3/source/Cargo.toml),
  [`one_d_reader.rs`](https://github.com/rxing-core/rxing/blob/main/src/oned/one_d_reader.rs),
  [rxing-wasm](https://github.com/rxing-core/rxing-wasm)),
  [zxing-cpp](https://github.com/zxing-cpp/zxing-cpp) and
  [its crate](https://crates.io/crates/zxing-cpp),
  [rqrr](https://crates.io/crates/rqrr),
  [bardecoder](https://crates.io/crates/bardecoder),
  [zbar-rust](https://crates.io/crates/zbar-rust),
  [zedbar](https://github.com/eventualbuddha/zedbar),
  [zxing-bench](https://github.com/axxel/zxing-bench/blob/main/rust/README.md)
  (by zxing-cpp's author);
  writers [barcoders](https://crates.io/crates/barcoders),
  [qrcode](https://crates.io/crates/qrcode),
  [datamatrix](https://crates.io/crates/datamatrix)
  ([its PDF example](https://github.com/jannschu/datamatrix-rs/blob/main/examples/pdf.rs));
  [USS-39](https://live.expresscorp.com/uploads/specifications/47/USS-39.pdf),
  [QR](https://en.wikipedia.org/wiki/QR_code) and
  [Code 39](https://en.wikipedia.org/wiki/Code_39)'s character sets.
  The bench: a scratch crate (rxing 0.9.3, zxing-cpp 0.5.3, rqrr 0.11.0,
  qrcode 0.14.1, datamatrix 0.3.3, image 0.25.10), its results kept in
  the planning session's scratchpad, not in the repo.
- **Patch codes and scanned barcodes:** Kodak A-61599,
  [Patch Code Information](http://www.alliancegroup.co.uk/downloads/patch-codes.pdf)
  (Alliance's copy), [Kodak's Patches for Printing](http://www.alliancegroup.co.uk/downloads/patches-for-printing-on-us-letter-paper.pdf),
  [Patch Code](https://en.wikipedia.org/wiki/Patch_Code) on Wikipedia,
  [twain.h](https://github.com/twain/twain-dsm/blob/master/TWAIN_DSM/src/twain.h),
  Kofax Scan Client's [supported barcodes](https://docshield.tungstenautomation.com/RSXBSC/en_US/5.10/help/XBSC/Content/other/Supported_barcodes.htm),
  [ABBYY's sample separator](https://help.abbyy.com/en-us/finereaderserver/14/admin_guide/howto_configuredocumentseparation/),
  [PaperStream Capture's datasheet](https://www.fujitsu.com/us/imagesgig5/PaperStream_Capture_Datasheet_v2.pdf),
  [CaptureOnTouch Pro](https://canon-europe.com/business/products/software/captureontouch-pro/specifications),
  [Kodak's barcode tips](https://resources.kodakalaris.com/support/Capture%20Pro%20Help/Content/Tips_for_using_bar_codes.htm),
  [SimpleOCR's sizing guidance](https://www.simpleocr.com/?p=6085) (third-party).
