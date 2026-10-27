# Build or adopt: decisions for Greg

Part of [build or adopt](README.md).

Each has its options, recommended first. Where the sections left a
choice open, the pick is this memo's and says so.

### D1: The Noto fallback: keep the glyph-ID rule? (M10 slice 7)

The plan says any font sheaf embeds keeps each glyph's number from the
whole font (`content/font/mod.rs:35-38`; m10.md:1455-1460), and
m10.md:633 names subsetter. subsetter always renumbers. Entry 17.

Greg named subsetter for the subsets sheaf writes (2026-10-24), after
this memo. Option 1 was added on 2026-10-25: it takes subsetter and keeps
what the rule is for, so it now comes first.

1. **Take subsetter, and map the old numbers to its new ones with a
   `/CIDToGIDMap` stream** instead of `/Identity`. Content still draws
   each glyph by its number in the whole font (CID = the whole font's
   GID). Only the font's map changes per subset, so codes stay writable
   before the subset is known, appearance digests stay independent of
   the save, and an Append's earlier subset stays valid.
   - **Cost:** the map is 2 bytes per CID up to the highest one drawn,
     at most 7,768 bytes for Noto's 3,884 glyphs before Flate. It's
     mostly zeros. subsetter's dense subset has no padded `loca` and
     `hmtx`. The `Font` docs keep the rule and change only its wording
     ("CID = the whole font's GID", not `/Identity`). A `/CIDToGIDMap`
     belongs to a Type 2 (TrueType) CIDFont, which Noto Sans is.
   - **Checked:**
     - subsetter 0.2.6's `GlyphRemapper::get(old) -> Option<u16>` gives
       the map, and `new_from_glyphs_sorted` makes it ascending
       whatever order the glyphs are met in.
     - PDFium's `CPDF_CIDFont::GlyphFromCharCode` reads an embedded
       font's stream map as 2 big-endian bytes per CID. A CID past the
       end draws no glyph.
   - **Unchecked:** the rendering itself, and the size after Flate.
     Both are for slice 7's checks, in PDFium, pdftoppm and pdf.js.
   - **Kept from entry 17:** subsetter's output may change across its
     releases, and that changes new subsets' `FontRes` digests.

   Smaller than option 2.
2. **Keep the rule with `/Identity`. Build a glyph-ID-preserving `glyf`
   subset in-house on read-fonts,** a port of M10.5's `gen.py`
   algorithm. Use skera with `SUBSET_FLAGS_RETAIN_GIDS` if it grows. It
   keeps codes writable before the subset is known, appearance digests
   independent of the save, and an Append's earlier subset valid. The
   cost: `loca` and `hmtx` at about 15.5 KB each before Flate. Medium.
   This was the memo's first recommendation.
3. Drop the rule and adopt subsetter 0.2.6 with `/Identity`. The
   emitter then picks codes per output (two passes), and M10.5's `Font`
   docs change.

### D2: M12's RustCrypto line: wait for the cms 0.3 round, or pin pre-releases?

cms 0.2.3, the only stable release, reads none of the corpus's 138 BER
signatures. cms 0.3.0-rc.0 (2026-10-13) reads all of them. x509-ocsp
0.3, x509-tsp 0.2 and pkcs12 0.2 aren't published, and p12-keystore
0.4.1 can't share a build with cms 0.3.0-rc.0. Entries 44, 48, 93.

1. **Don't hold work for the round.** Build B5 on cms 0.3.0-rc.0 now,
   pinned exactly. At M12's kickoff, re-check the line: take the
   published round if it's out; otherwise pin the pre-release set M12
   needs, exactly. p12-keystore waits for the published cms 0.3.
   Medium-high.
2. Hold M12 until the round is published.
3. Stay on cms 0.2.3, or use openssl. No: 0.2.3 can't read a quarter of
   real signatures, and openssl puts a C parser in the path of untrusted
   input.

### D3: veraPDF in tests and CI: Java or Docker? (M14, M17, F9)

Nothing in Rust validates PDF/A or PDF/UA near veraPDF's depth.
veraPDF is GPLv3+ or MPLv2+, so it runs as a separate tool, never
linked. Entries 68, 82.

1. **Java plus the veraPDF 1.30 CLI** wherever the tiers run: a corpus
   tier over `conformance/` comparing sheaf's checks with veraPDF's JSON,
   and a tier that runs veraPDF on what sheaf converts. Each skips
   without it, as corpus tiers do without the corpus. In Linux CI, run it
   on sheaf's own written outputs. Medium.
2. The `verapdf/cli` Docker image. The newest tag a search showed was
   v1.28.2, possibly stale.
3. Rust-only checks, with no outside validator.

Also at M14's kickoff: whether sheaf validates other tools' files for
users. If so, veraPDF as an optional external tool or plugin, not a rule
engine written in Rust.

### D4: One Flate backend for the workspace

The shipped `sheaf` binary compresses with zlib-rs, because sheaf-app's
`zip` turns on `flate2/zlib-rs`. The `sheaf-cli` binary, per-crate test
runs and builds without plugins use miniz_oxide. The two give different
bytes at levels 1, 6 and 9. Entry 4, B26.

1. **zlib-rs, named once:** `flate2 = { version = "1", default-features
   = false, features = ["zlib-rs"] }` in the workspace, used by every
   crate. It's what ships today. Check whether anything replays a memory
   source across builds or versions and expects the same BlobId. High on
   one backend; medium on which.
2. miniz_oxide everywhere (flate2's default). zlib-rs has about 456
   `unsafe` in its `src/`; with miniz_oxide, the shipped app's output
   changes instead.
3. Leave it to feature unification.

**Ruled 2026-10-27 (Greg): 1, zlib-rs.** "go with both recommendations":
zlib-rs, named once in the workspace, in every sheaf build, for speed.
pdf-filters supports both backends, the caller naming one, and pdf-lint
keeps miniz_oxide (M18, slice 6); sheaf names zlib-rs at every call
once it compresses through pdf-filters (M18 slice 12). The check on
memory sources goes with [B26](../../backlog/B26.md)'s fix.

### D5: M11's image decoding (slice 6)

PDFium can't decode for re-encoding: `FPDFImageObj_GetBitmap` converts to
BGR or gray, and `GetImageDataDecoded` stops at DCT, JPX, JBIG2 and
CCITT. Entries 33–38.

1. **One sample decoder in `sheaf-image`** (pixels in, pixels out, behind
   a small trait): lopdf for the stream filters, zune-jpeg for DCT, and
   hayro-ccitt, hayro-jbig2 and hayro-jpeg2000. PDFium stays the
   renderer that checks the output. High on the shape; medium on
   hayro-jbig2, "under development".
2. PDFium's bitmap: every redacted image becomes DeviceRGB or
   DeviceGray, spot colors are lost, and a PDF/A file can stop
   conforming.
3. C codecs (openjpeg-sys, turbojpeg): C parsing untrusted images, where
   the pure-Rust crates are safe at the crate level.

With it, three smaller asks:
- **Re-encoding:** JPEG with jpeg-encoder (CMYK, the source's tables, no
  APPn carried over; an IJG notice); JPX becomes Flate; JBIG2 becomes G4;
  JBIG2 is never written. Recommended, medium-high.
- **A JBIG2 globals stream another image shares** (15 of the corpus's
  127 JBIG2 scans share one): its symbols can include glyphs only the
  redacted area used. The section left it open. **This memo's pick:**
  re-encode every image that shares it and drop the stream, and say so
  in the report. The other option is to keep it, after deciding it
  leaks nothing.
- **The report** says which images were re-encoded, since a re-encoded
  JPEG loses quality over the whole image.

### D6: Clipping line art: i_overlay directly, and strokes (M11 slice 6)

m11.md:160-163 names "`geo` or `i_overlay` (licenses to check)". Both are
MIT OR Apache-2.0. geo 0.33.1 pins i_overlay `>=4.5.1, <4.6.0`, five
majors back. Entry 31.

1. **i_overlay 9.x directly**, pinned exactly, behind a one-function
   sheaf-doc module (path minus marks), with kurbo for flattening and
   strokes. clipper2-rust (BSL-1.0) runs beside it in the corpus tier as
   a differential check. Coordinates are checked (finite, bounded) and
   segment counts capped first, because the float API panics. Medium.
2. geo's `BooleanOps`: an old i_overlay and a GIS API, for no gain.

And strokes, which the section left for kickoff:
1. **This memo's pick: expand the stroke to its outline with kurbo**
   (joins, caps, miter limit, dashes), subtract the marks and fill the
   rest. It's exact. The cost: a stroke becomes a fill, which M13's
   editing then sees.
2. Clip the centerline (i_overlay's inverted string clip, or clipper2's
   open subjects) against marks grown by half the line width under the
   CTM and the miter limit. The stroke stays a stroke, with new butt
   ends at the cuts, but the growth has to be right for every join.

### D7: An advisory and license check in CI

sheaf has none: no `deny.toml`, nothing in `.github/`. M12 brings crypto
crates with live advisories (rsa: RUSTSEC-2023-0071, unpatched), and
this audit checked licenses by hand. B27.

1. **`cargo deny check advisories licenses`**, with a reviewed ignore
   list. Bans later. Small.
2. `cargo audit` (advisories only).
3. Keep checking by hand.

### D8: The macOS keychain in M12

M12 names the Windows store and PKCS#11 tokens (ROADMAP.md:397-398), not
the keychain, though macOS is a target platform. Entry 51.

1. **Add it,** on security-framework 3.7.0 (already in the lockfile):
   identities, `SecKeyCreateSignature`, trust settings. High.
2. macOS signs only with PKCS#12 files and tokens.

### D9: The field editor's painting (M10 slice 11)

The plan paints a field with no Liberation match "in that embedded font
if GPUI can load it" (m10.md:624-627). That hands untrusted font bytes
to CoreText or DirectWrite, C and C++ stacks with a history of font
CVEs, and registers the font for the process's lifetime. Entry 20.

1. **Paint sheaf's own glyph runs with GPUI's `Window::paint_glyph`**,
   from slice 6's layout on the bundled fonts, so the editor matches the
   appearance by construction. Never `add_fonts` a PDF's program: draw
   its skrifa outlines as GPUI paths, or fall back to Liberation Mono or
   Sans. Medium: `paint_glyph` with fonts from `add_fonts` is unverified
   on macOS and Windows, so spike it first.
2. The plan as written.

### D10: M14's embed-all substitutes for Symbol and ZapfDingbats

`Profile.fonts: EmbedAll` embeds a substitute for every standard 14 font
sheaf writes, ZapfDingbats check boxes included. Liberation covers only
Helvetica, Times and Courier. Entry 63.

1. **Foxit's FoxitSymbol.pfb and FoxitDingbats.pfb** (Type 1, BSD-3, the
   PDFium Authors), which PDFium draws for them and pdf.js ships. Medium.
2. Leave those two unembedded, and the profile fails on them.

### D11: Blank redacted glyphs' outlines in M11, not M14 (M11 slice 5)

Today a font subset in the redacted copy can show that a character
occurred, though not where, until M14 re-subsets (m11.md:562-564). Slice
5 already computes the codes "no longer used anywhere" for `/ToUnicode`
and `/Widths`. Entry 28.

1. **Blank their outlines in slice 5:** a zero-length `glyf` entry, an
   `endchar` charstring in CFF, the same under eexec in Type 1. Codes,
   widths and content streams don't change. That closes the documented
   leak, and M14 reuses it. Medium.
2. Keep the leak documented until M14.

### D12: XMP: one writer or two, and a PDF/A file's claim on sanitize

M11 slice 9 removes metadata, which voids a PDF/A or PDF/UA claim held
in XMP. M14 and M17 write XMP that keeps a file's other schemas. The
sections split: security picked xmp-writer for M11's fresh packet
(high), core picked quick-xml for M14's patching, warning that two
writers mean two code paths (medium). Entry 41.

- **The claim:**
  1. **Keep it:** write a minimal fresh packet with `pdfaid` and
     `pdfuaid`, Producer and ModifyDate.
  2. Say the claim goes.
- **The writer, this memo's reconciliation:**
  1. **One path on quick-xml** (read, patch, write) for both, with
     xmp-writer's schema coverage as the checklist. M14 has to patch
     existing packets anyway, and M11's packet is "keep only these,
     set those".
  2. xmp-writer for M11's fresh packet, and patching added in M14.

### D13: M12's other kickoff asks

Entries 45, 47, 52, 55, 56.
- **Signing with PKCS#12 keys:**
  1. **ring** (constant-time, already in sheaf-app's build) wrapped in a
     `signature::Signer` adapter for cms's builder.
  2. the rsa crate, whose Marvin advisory (RUSTSEC-2023-0071) is still
     unpatched. Fine for local use, but an audit tool would flag it.
- **Trust anchors:**
  1. **User-trusted certificates plus the OS stores** to start, the EU
     Trusted Lists later. Low confidence.
  2. Also the Adobe Approved Trust List, if third parties may use it
     (unverified).
- **Timestamps and revocation:**
  1. **x509-tsp 0.2 and x509-ocsp 0.3 once published, with a small
     synchronous, bounded HTTP client;** tsp-ltv as prior art and an
     oracle. Which client is open: the installer picked ureq
     (`plan/topics/installer.md`), and hyper, tokio and rustls are already
     linked.
  2. Adopt tsp-ltv, if it moves to the 0.3 line and lets callers supply
     the transport.
- **Chain validation:**
  1. **rustls-webpki behind a trait,** with sheaf's PDF policy (EKU,
     signing time, DSS revocation, legacy algorithms) on top.
  2. synta-x509-verification (RFC 5280 §6, pluggable crypto), if it has
     matured by kickoff; it's young.
- **The typed-signature font:** pick an OFL or Apache script font, its
  license checked, embedded through M10 slice 7's subsetting.

### D14: The PDF/A output intent profile (M14)

The sections split. Core: take krilla's CC0 profiles from
Compact-ICC-Profiles. Images: moxcms (already built) can generate an sRGB
profile, so no third-party file is needed. Entries 40, 68.

1. **This memo's pick: the CC0 Compact-ICC-Profiles** (`sRGB-v4.icc`,
   `sRGB-v2-magic.icc`, the gray pair), which krilla ships and checks
   against veraPDF. Which part needs v2 or v4 is unverified; settle it
   with veraPDF at kickoff.
2. A profile generated by moxcms. moxcms stays either way, for
   converting fill colors into an image's color space.

### D15: Spell check engines (Pending)

Entry 84.
1. **Platform checkers on Windows and macOS, spellbook on Linux,** with
   SCOWL's en_US and a tax word list bundled; drop zspell. Medium.
2. spellbook everywhere, if firms want identical suggestions on every
   OS.

spellbook is MPL-2.0: sheaf must offer its source.

### D16: M16 on Windows: bridge threads or a per-user named pipe? (agent.md decision 2)

Entries 78, 80.
1. **tokio's named pipes** (first instance, reject remote clients, a
   user-only security descriptor), with the SID check before any data
   flows: about 50 lines on the windows crate, which no crate does.
   interprocess 2.4.6 if a synchronous API is wanted. Medium.
2. `uds_windows` plus the `SIO_AF_UNIX_GETPEERPID` ioctl, which keeps
   today's socket. Microsoft's documentation for the ioctl wasn't found.

### D17: Copyleft loaded at run time (M5)

The rule says never link copyleft. M5's plan ships the TWAIN DSM
(`TWAINDSM.dll`, LGPL-2.1) unmodified and loads it at run time
(`m5-intake.md:660-662`). A Flatpak would bundle sane-backends (GPL-2.0+
with the SANE exception), loaded through libsane. Entries 5, 6.

1. **Allowed:** loading an unmodified, separately shipped library at run
   time isn't copying, translating or linking sheaf against it. Ship
   its license text and a source offer, as M5 plans.
2. Not allowed: users install TWAIN DSM and sane-backends themselves,
   and the Flatpak has no scanning.

**Ruled 2026-10-23 (Greg): 1, allowed.** "The linking rule for dynamic libs
is fine." The scanner slices that use it (M5 slices 6 and 8) are deferred
until there are scanners to test against.
