# M5 slice 8: Windows: TWAIN

Status: deferred · Part of [M5: Scan & OCR](../README.md)

**Windows: TWAIN** (proposal 7; asks 3 and 13).
- **Builds:** first the spike: TWAIN's sample data source installed
  silently on `windows-latest`. Then the `twain` backend, the 32-bit
  worker (needed: the office's fi-6130s have a 32-bit TWAIN driver,
  ask 12), `TWAINDSM.dll` bundled, Scanner settings… (the driver's
  window).
- **Checks:** with the spike working, CI scans 10 images through the
  sample source in both bitnesses; the error mapping in unit tests;
  by hand, an fi-6130 on FUJITSU TWAIN32 through the 32-bit worker,
  duplex, with a jam and a double feed (and on PaperStream IP x64 if
  the office's are fi-6130Z), once a build may run on a work PC (ask
  12).
