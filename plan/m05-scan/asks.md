# M5: Asks for Greg's kickoff

Part of [M5: Scan & OCR](README.md).


Each with its options, the recommended answer first.

1. **Where does Create from Scanner (and from Scans) put its pages?**
   (proposal 2)
   - **An untitled document in a new window, pages streaming in;
     nothing written until Save, which asks where; its recovery sidecar in
     the state folder.** Recommended: you see pages as they come, a crash
     after the first batch loses nothing, and it fixes B11.
   - Ask for the path first, scan inside the dialog, save, then hand the
     Session to a window (Combine's way). Less app work; nothing visible
     until the end, nothing recoverable before it.
2. **Which comes first, separators or scanners?**
   - **Separators first** (slices 3–5), then scanners (6–9).
     Recommended: separators need no hardware, serve every way scans
     arrive today (a copier's PDFs, a ScanSnap's files through Send to,
     image files), and carry the PDFlyer parity row; the scanner slices
     carry the platform risk and need checks by hand.
   - Scanners first, as the roadmap lists them.
3. **Which driver APIs on Windows?** (proposals 5, 7, 8)
   - **TWAIN and WIA, TWAIN first, with a 32-bit worker for 32-bit
     TWAIN drivers.** Recommended: office document scanners and tax
     software use TWAIN; WIA comes with Windows and is how network
     copiers appear; NAPS2 found TWAIN drivers "generally 32-bit only",
     and the worker makes a 32-bit build cheap.
   - TWAIN and WIA, 64-bit TWAIN only: one less build and four fewer
     files, and PaperStream IP, Epson Scan 2 and newer Canon drivers have
     64-bit TWAIN; older drivers then don't work.
   - WIA only (the roadmap's first word): fewest moving parts, but no
     PaperStream, and most office scanners' best driver unused.
   - TWAIN only: no network copiers without a TWAIN driver.
4. **macOS?** (the roadmap names nothing; macOS is tier 2)
   - **Not in M5: a Pending entry, "Scanning on macOS
     (ImageCaptureCore)".** Recommended: tax offices scan on Windows,
     nothing here can check a Mac scanner, and the dialog says scanning
     isn't on macOS yet (files, ScanSnap's Send to and separators all
     work).
   - An ImageCaptureCore slice after Windows, behind a cut line, through
     `objc2-image-capture-core` (0.3.2, which has `ICScannerDevice` and
     the feeder unit): it reaches Mac scanner drivers and AirScan copiers
     in one API, checked in CI only as far as it builds, and by hand on a
     Mac.
   - sheaf's own eSCL client: network scanners only, a Local Network
     privacy prompt on macOS 15 and later, and Mopria's specification
     licensed for internal implementation only.
5. **Drivers in a worker process?** (proposal 4)
   - **Yes, always.** Recommended: a driver's crash or hang never takes
     unsaved documents with it, and 32-bit TWAIN needs it.
   - In process on a thread of its own: simpler; a driver crash is
     sheaf's crash; no 32-bit TWAIN.
6. **libsane linked or loaded at run time?** (proposal 6)
   - **Loaded at run time** (`dlopen` of `libsane.so.1`), with sheaf's
     own bindings. Recommended: sheaf starts without SANE installed, as
     it does without ONNX Runtime, and nothing of SANE is shipped.
   - Linked: a build-time dependency on SANE's headers and a binary that
     won't start without libsane1.
7. **Which barcode library?** (proposal 15)
   - **rxing, behind a small trait in `sheaf_image::codes`.**
     Recommended: pure Rust (no new C++ build), Apache-2.0, reads and
     writes every symbology needed, and it read every sheaf-sized QR in
     the bench's worst cases; the trait lets zxing-cpp in later if real
     scans show misses, as the OCR trait let two engines in.
   - zxing-cpp: more thorough (159 of 170 against 149) and about 1.6
     times faster, but a C++20 library built with CMake in every build
     (CI already builds Tesseract that way).
   - zbar: LGPL, no Data Matrix or PDF417, and dropped by Paperless-ngx
     as unreliable on skewed and faint codes.
8. **What symbology does sheaf's own sheet use?** (proposal 13)
   - **QR, two copies, 0.75 mm modules, error correction M; patch T bars
     optional, off.** Recommended: reads at any rotation, holds a UTF-8
     title, reads in 1-bit at 150 dpi, and is the 2D symbology TWAIN
     drivers know.
   - Data Matrix: smaller for the same text and as robust, but no TWAIN
     driver reads it.
   - Code 128 (SurePrep's): read by every scanner's firmware, but one
     line of bars can't hold a long title at a size that reads at 200
     dpi (25 characters at 28 mil is wider than Letter).
   - Patch T on by default: every sheet then splits in other capture
     software too, which may not be what the firm wants.
9. **Which other separators are read?** (proposals 12, 14)
   - **Patch codes of every type by sheaf's own reader (T, 2 and 3 act
     by Kodak's convention, 1, 4 and 6 are listed only), Kodak's and
     NAPS2's Code 39 `PATCH…` text, and rules for Code 39, Code 128, QR,
     Data Matrix and PDF417 by pattern, and keywords.** Recommended:
     covers every sheet the research found in use (PaperStream's,
     Kofax's, Kodak's, Alliance's, SurePrep's, a firm's own), and
     patterns keep content codes out.
   - Patch T only, and sheaf's own sheets: smaller, but firms with other
     sheets keep their other software.
   - Also ITF, Codabar and the retail codes: the bench read junk of
     those off real scans; none is a separator anyone prints.
10. **What does sheaf's sheet encode?** (proposal 13)
    - **The title, the action (bookmark, split, both), the level (top, or
      under the last top one), and whether the sheet is dropped.**
      Recommended: what the separation needs, and the format takes more
      fields later without breaking older readers.
    - Also a client, a year and a document type, for naming files
      (`{client} {year} {title}`). Useful once sheaf has client fields
      to fill them from; until then the title carries them.
    - A one-time id, as GoFileRoom's cover sheets have: needs a
      database, which a desktop editor doesn't have.
11. **Is separation core or a plugin?** (proposal 20)
    - **Core,** with plugins seeing its results and its sheets as an op
      when inserts open to them. Recommended: it needs page inserts and a
      new page source, which 0.4 doesn't give plugins, and it's part of
      the scan path and the command line.
    - A bundled plugin, after a plugin API version that can insert
      pages: waits on that API; can't run inside a scan.
12. **Which scanners can be checked by hand, and what do the pilot
    offices use?** **Answered (Greg, 2026-10-23):** none at home; at
    work, several Fujitsu fi-6130 duplex color scanners, "old, but very
    good". Older hardware like this often works better than newer
    models, if sheaf can talk to it, so old drivers are a main case, not
    a fallback.
    - **The fi-6130** (2008; 40 ppm, 50-sheet feeder, ultrasonic
      double-feed detection) predates PaperStream IP. Its Windows driver
      is FUJITSU TWAIN32 (9.21.x), a 32-bit TWAIN source, so the 32-bit
      worker (ask 3) is needed for the office's own scanners. The
      fi-6130Z (2012) has PaperStream IP TWAIN x64 up to 3.10 only; 3.20
      dropped the Z models. Still to learn: which model, and which driver
      is installed.
    - **SANE's fujitsu backend** rates the fi-6130 and fi-6130Z
      complete, so a Linux laptop at work covers slice 6's hand checks.
    - **Later models:** fi-7140/7160 (PaperStream IP, 64-bit TWAIN),
      fi-8170 (70 ppm, 140 ipm duplex, USB 3.2 and LAN; SANE "good",
      Ethernet not supported). The scan-speed gate (slice 6) is sized for
      the fi-8170.
    - **Open:** whether a development build may run on a work PC (IT and
      unsigned programs; `plan/topics/code-signing.md`), which decides when the
      Windows hand checks can happen.
13. **CI for scanners.** (proposal 22)
    - **Linux runners install `libsane1` and run SANE's test backend
      and sheaf's fake libsane; the TWAIN slice starts with a spike to
      run TWAIN's sample data source on the Windows runner; WIA is
      checked by hand.** Recommended.
    - Fakes only: faster CI, but the real SANE library and a real TWAIN
      source never run outside a person's machine.
14. **What do other software's separators do by default, and how are
    split files named?** (proposal 16)
    - **Bookmark, separator dropped, separators starting their section
      (not ending it); split files named by their titles alone.**
      Recommended: a binder with bookmarks is how CCH and PDFlyer users
      work; capture software names files from the sheet; trailers
      (`Place::Ends`) are a rule's option for firms whose sheets go
      after each document.
    - Split by default; files named `{stem}_{title}` like Split
      Document's parts.
