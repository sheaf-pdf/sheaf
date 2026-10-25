# Build or adopt 68: PDF/A conversion and validation; preflight-lite

Area: M14: Document processing · Part of [build or adopt](README.md)

- **Need:** convert to PDF/A (embedded fonts, an output intent with an
  ICC profile, XMP agreeing with Info, no forbidden features: JavaScript,
  encryption, LZW, transparency in PDF/A-1); validate a file's PDF/A
  claim; preflight-lite checks (font embedding, color spaces,
  transparency).
- **Where:** `ROADMAP.md:455-456`; M10.5 §20
  (`m10.5-edit-api.md:3669-3670`, fonts at `:3665-3668`); M10.5's D76
  (`m10.5-decisions.md:1785-1791`); F9 (`backlog.md:665-686`). D3, D14.
- **Today's plan:** "PDF/A convert and validate ❓" and "Preflight-lite …
  ❓" (`ROADMAP.md:455-456`), how far to go decided at kickoff
  (`:460-461`). M10.5 adds only the Note.
- **Candidates:**
  - **veraPDF** ([library](https://github.com/veraPDF/veraPDF-library),
    [apps](https://github.com/veraPDF/veraPDF-apps)): Java, GPLv3+ or
    MPLv2+ (library README). The latest on Flathub is 1.30.1, listed "6
    months ago" ([Flathub](https://flathub.org/apps/org.verapdf.veraPDF));
    its changelog has PDF/UA-2 and WTPDF fixes and adds Java 25; 1.31.x
    tags are development builds. The CLI takes `--flavour` with
    `1a`…`4f`, `ua1`, `ua2`, `wt1r` and `wt1a`, and `--format json`, and
    `--recurse`s into directories
    ([CLI docs](https://docs.verapdf.org/cli/validation/)); exit codes
    aren't documented. Docker [`verapdf/cli`](https://hub.docker.com/r/verapdf/cli):
    the newest tag a search showed was v1.28.2, possibly stale. No Java
    on this machine today.
  - **pdf_oxide 0.3.78** `src/compliance/`: `pdf_a.rs` 7.5 KB,
    `validators.rs` 24 KB, a 93 KB PDF/X validator; its converter falls
    back to a built-in sRGB profile stored as
    `srgb_profile_placeholder.bin`. Shallow next to veraPDF's rule
    profiles.
  - **oxidize-pdf 5.5.1:** its own claims list says PDF/A validation is
    "Basic functionality … not independent certification", and makes "no
    … PDF/UA compliance" claim (`docs/CLAIMS.md:40,51`). GPL, then AGPL,
    then MIT.
  - **The Arlington PDF Model**
    ([pdf-association/arlington-pdf-model](https://github.com/pdf-association/arlington-pdf-model)):
    Apache-2.0, pushed 2026-10-21. TSV files describing every PDF 2.0
    object, with version predicates. TestGrammar (C++) is its proof of
    concept; veraPDF publishes an Arlington service (entry 94).
  - **ICC data:** [Compact-ICC-Profiles](https://github.com/saucecontrol/Compact-ICC-Profiles)
    is CC0-1.0 ("released to the public domain"). krilla ships its
    `sRGB-v4.icc`, `sRGB-v2-magic.icc`, `sGrey-v4.icc` and
    `sGrey-v2-magic.icc` (`crates/krilla/icc/`). Which one each PDF/A
    part needs (v2 or v4) is unverified. (The images section would
    generate the profile with moxcms instead: D14.)
  - **Data:** `../pdf-corpus/conformance/` has 3,524 files from the bfo
    and veraPDF suites, among others; its manifest gives the expected
    result and clause per file (`expect`, `profile`, `clause`): ground
    truth for any validator.
- **Fit:** no Rust crate validates foreign files near veraPDF's depth.
  Conversion is sheaf's own, on its emitter (fonts: entry 63; images:
  entries 33–38, 65; XMP: entry 41). Bundling veraPDF in the app would
  mean shipping a JRE: the MPL option allows it, but it's heavy and
  foreign to sheaf's stack.
- **Recommendation:** **build in-house** for conversion and for the rules
  covering what sheaf writes, modeled on krilla's clause tables. Use
  **veraPDF as a test tool, never linked**: a corpus tier over
  `conformance/` comparing sheaf's checks with veraPDF's JSON, and a tier
  running veraPDF on sheaf's converted output. Medium. User-facing
  validation of other tools' files: M14's kickoff decides; if wanted,
  veraPDF as an optional external tool or plugin, not a rule engine in
  Rust.
