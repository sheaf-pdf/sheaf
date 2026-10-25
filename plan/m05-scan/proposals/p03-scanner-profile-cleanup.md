# M5 proposal 3: A scanner profile for cleanup

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**A scanner profile for cleanup.** `ScanParams::default()` is a phone
photo's: find the page in the frame, even out the lighting. A
scanner's page needs neither: the page is the image, the light is
even, and the page finder can crop a white page on a white lid wrongly
(to check in slice 1; it didn't on slice 1's flatbed scan, where the
light evened out is the difference: finding 6). So cleanup gets two
profiles, each row still switchable:

| Step | Photo (files; today's default) | Scanner (devices) |
|---|---|---|
| Find the page, clear its edges | on | off |
| Flatten curl (UVDoc) | off | off |
| Even out lighting, white paper | on | off |
| Turn upright | on | on |
| Straighten | on | on |
| Leave out blank pages | off | on with duplex, else off (files: off) |
| Black and white | Sauvola on the cleaned page | the same: sheaf asks the scanner for gray and thresholds it, so every scanner's black and white looks alike |

`sheaf scan --profile photo|scanner` (photo for files, scanner for a
device). A folder of TIFFs from a copier's scan-to-folder is a
scanner's pages: `--profile scanner` says so.
- Alternative: one profile and let the user switch steps. Every
  scanner user would have to turn the same two off.
- Asking a scanner for gray and thresholding in sheaf costs 8 times
  the bytes of 1-bit over USB or the network (about 8 MB a Letter page
  at 300 dpi, raw). That's well within USB 2's and gigabit's rate
  at a feeder's 30–60 pages a minute, but slow on Wi-Fi; a later
  option can ask for the scanner's own black and white.
