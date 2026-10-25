# Build or adopt 61: Font matching, installed fonts and fallback

Area: M13: Content editing · Part of [build or adopt](README.md)

- **Need:** find an installed font to substitute for a subset, or for a
  character no bundled font covers; map PDF names and descriptor flags to
  candidates; respect OS/2 `fsType` embedding permissions.
- **Where:** M13 (`ROADMAP.md:418-420`); M10 "Installed fonts … (found
  with `fontdb`, embedded where the font allows): later"
  (`m10.md:2049-2051`); M14 (`m10.5-edit-api.md:3663-3668`).
- **Today's plan:** fontdb named for later; matching not designed.
- **Candidates:**
  - **[fontdb](https://crates.io/crates/fontdb) 0.24.0** (2026-07-29;
    MIT; 40M downloads; one release in 12 months): pure Rust; scans font
    directories and parses fontconfig's config files (not
    libfontconfig); its `memmap` feature uses `unsafe` mmap. **0.23 is
    compiled on every platform already.**
  - [fontique](https://crates.io/crates/fontique) 0.12.0 (Apache-2.0 OR
    MIT; linebender; 7 releases in 12 months): native enumeration through
    DirectWrite, CoreText and fontconfig (`fontconfig-dlopen`), on
    read-fonts 0.41. More faithful to what the OS calls installed
    (per-user fonts, activation).
  - [font-kit](https://crates.io/crates/font-kit) 0.14.3 (2025-05-26; MIT
    OR Apache-2.0; servo): no release in 12 months; FreeType and native
    loaders. GPUI's Linux build carries a Zed fork.
- **Fit:** these enumerate and give file bytes; matching is sheaf's.
  pdf.js's font alias tables (Apache-2.0, e.g. ArialMT → Helvetica) and
  PDFium's `CFX_FontMapper` (BSD-3) are the references, with
  attribution. Whether fontdb finds Windows' per-user fonts
  (`%LOCALAPPDATA%\Microsoft\Windows\Fonts`) is unverified.
- **Recommendation:** **build matching in-house; adopt fontdb** for
  enumeration. Switch to fontique if fontdb misses user-installed fonts
  on Windows or macOS. Medium.
