# Build or adopt: changes to make, by file

Part of [build or adopt](README.md). Line numbers here refer to the files as of the audit (2026-10-23), before the plan moved out of `plan/`.

Every plan-line change the four sections proposed, merged. Line numbers
are on `8cdc589` (the M5 plan: `m5-intake` at `4481573`). **This PR
edits no plan,** because other open PRs touch them: the content
interpreter PR edits `plan/m11-security/README.md` and `ROADMAP.md`; #31 edits
`ROADMAP.md`; #32 edits `ROADMAP.md`, `plan/m16-agent/README.md`, `README.md` and
`Cargo.toml`, so lines in those move a little. Each change is
applied at the kickoff of the milestone it belongs to (for M11, which is
under way, at the start of the slice), or by the PR named. Changes that
hang on a decision say which.

### `plan/m10-forms/README.md`: at M10's kickoff (no open PR edits it)

- **`:590-595` (stale):** M10.5 slice 6 built all 14 AFM tables
  (`content/font/afm.rs`, `std14.rs`, `assets/afm`), and
  `stamps.rs:207-254` no longer exists. Say "built in M10.5" instead of
  "are new".
- **`:607-611` (stale):** the OCR layer's Type 0 dictionaries are now
  `content/font/glyphless.rs`, not `text_layer.rs:395-500`.
- **`:624-627`:** drop "painted in that embedded font if GPUI can load
  it". A PDF's font program isn't handed to CoreText or DirectWrite:
  draw its skrifa outlines as GPUI paths, or use the Liberation Mono or
  Sans fallback. (D9.)
- **`:632-635`:** replace "(for example `subsetter`, `ttf-parser`,
  `unicode-normalization` and `unicode-segmentation`; licenses to
  check)" with: skrifa/read-fonts for font programs (TrueType, CFF and
  Type 1); a GID-preserving `glyf` subset, in-house on read-fonts or
  skera with `RETAIN_GIDS`, not subsetter, which renumbers glyphs (D1);
  unicode-normalization, already sheaf-doc's; unicode-segmentation,
  already sheaf-ui's; unicode-bidi for the right-to-left check.
- **`:1455-1460` (slice 7):** name the subsetter decision (D1). Add: Noto
  Sans unhinted (431 KB, 3,884 glyphs), OFL with no Reserved Font Name.
- **`:1469` and `:1597`:** `packaging/licenses` gains `LICENSE-noto`
  (OFL), `LICENSE-liberation` (OFL; RFNs "Liberation", "Arimo",
  "Tinos", "Cousine"; shipped unmodified), and the Unicode-3.0 notice
  for unicode-normalization ≥ 0.1.26 and ICU4X data (F18). (Adobe's CMap
  notice lands with the content interpreter PR.)
- **`:1584-1590` (slice 11):** after "paints it by proposal 11's
  editor-font rule", add "as glyph runs from slice 6's layout through
  `Window::paint_glyph`, so the editor and the appearance place every
  glyph alike". (D9.)

### `plan/m11-security/README.md`: after the content interpreter PR merges, at each slice's start

- **`:459-463` (slice 3, Fonts):** add font programs through skrifa where
  the descriptor lacks metrics (PDFium's `CheckFontMetrics` takes
  `FontBBox`, Ascent and Descent from the program, or from the boxes of
  'A' and 'g'; `LoadFontDescriptor` negates a Descent above 10);
  predefined CMaps through hayro-cmap for Type0 fonts not on Identity
  (code length, code → CID for `/W`, CID → Unicode through
  Adobe-\*-UCS2); a corpus check on a CJK file. **Who:** the content
  interpreter PR, whose wrap-up takes skrifa and hayro-cmap; check it
  carries the `CheckFontMetrics` rule and the CJK check.
- **`:72-73`** (decision 3's "certificate stores (PKCS#12, the Windows
  store, PKCS#11)"): add the macOS keychain. **Who:** with the ROADMAP
  change at M12's kickoff (D8).
- **`:389-391`** ("#584 is left for slice 1") **and `:424`** ("Work
  around lopdf #584 for reading"): stale. The fix is upstream `0e781ce`
  (PR #573), in the vendored `39492cb` and in lopdf 0.46.0
  (`EncryptionState::decode` calls `resolve_password_for_key_derivation`,
  `vendor/lopdf/src/encryption.rs:623-624`). Keep only the check. #603 is
  also fixed upstream, in 0.46.0 (`2f497f8`). Issue #584 still shows
  open on GitHub and could be closed. **Who:** the next PR that edits
  m11.md.
- **`:425` and its Check at `:429-432`:** test the certificate-encryption
  refusal on the corpus's 12 `/Adobe.PubSec` files
  (`signatures/pyhanko/pdf/*pubkey*`, `signed-encrypted-pubkey-*`).
  **Who:** the next PR that edits m11.md.
- **`:488-493` (slice 5), `:562-564`, and the docs line `:536-537`:**
  blank the outlines of glyphs whose codes are "no longer used anywhere"
  (D11). **Who:** slice 5's start, if D11 is accepted.
- **`:152`:** replace "PDFium decodes, since it handles every filter."
  with: "sheaf decodes the samples itself, in the image's own color space
  and bit depth: lopdf for the stream filters, zune-jpeg for DCT,
  hayro-ccitt, hayro-jbig2 and hayro-jpeg2000 for the rest. PDFium's
  `FPDFImageObj_GetBitmap` converts to BGR or gray, and
  `GetImageDataDecoded` stops at image filters. PDFium renders the result
  as the check." (Both the images and security sections; D5.) **Who:**
  slice 6's start.
- **`:152-154`:** "JPEGs become JPEG again" says how: with `jpeg-encoder`
  (CMYK, the original's quantization tables and subsampling), with no
  APPn segments carried over. Add "JPX becomes Flate; JBIG2 becomes G4;
  JBIG2 is never written", and that the report says which images were
  re-encoded. **Who:** slice 6's start (D5).
- **`:155-157`:** add "An image's `/JBIG2Globals` goes when nothing else
  uses it; when another image shares it, [D5's answer]. 15 of the
  corpus's 127 JBIG2 scans share globals." and "The fill is converted
  into the image's color space, honoring `/Decode`." **Who:** slice 6's
  start.
- **`:160-163`:** replace "with a polygon-boolean crate such as `geo` or
  `i_overlay` (licenses to check)" with: i_overlay 9.x (MIT OR
  Apache-2.0; i_float and i_shape MIT), used directly, not through geo,
  whose 0.33.1 pins i_overlay 4.5; kurbo, already in the lockfile, for
  flattening and stroke expansion; clipper2-rust (BSL-1.0) as a second
  engine in the corpus check; coordinates checked (finite, bounded) and
  segment counts capped before the float API, which panics otherwise;
  and D6's answer on strokes. (The security section; core's license note
  is the same.) **Who:** slice 6's start.
- **`:497-505` (slice 6):** add "the image sample decoder (a
  `sheaf-image` module, also used by M13, M14 and M5's separator
  fallback)" to the slice, and make its check read the image data back
  through that decoder. **Who:** slice 6's start.
- **`:242`:** metadata. Removing XMP from a PDF/A or PDF/UA file voids
  its claim: write a minimal fresh packet keeping `pdfaid` and `pdfuaid`,
  with Producer and ModifyDate, or say the claim goes (D12). Read it
  with quick-xml, as sheaf-doc does now. **Who:** slice 9's start.
- **`:250-251`:** actions and JavaScript: add `/AA` on pages, annotations
  and fields; follow `/Next` chains; add SubmitForm, ImportData, GoToR,
  GoToE and RichMedia; XFA `<script>` elements in the template packet
  (quick-xml), unless the form-fields category removes XFA. **Who:**
  slice 9's start.

### `ROADMAP.md`: at each milestone's kickoff (the content interpreter PR, #31 and #32 edit it now)

- **M12, `:396` (Fill & Sign):** add "a bundled script font for typed
  signatures (license to check), embedded through M10 slice 7's
  subsetting".
- **M12, `:397-398` and slice 4 at `:404-405`:** add the macOS keychain
  (security-framework) beside the Windows store and PKCS#11 tokens (D8).
- **M12, `:401`:** LTV comes in two forms, and verification reads both:
  the PDF `/DSS` with `/VRI` (PAdES B-LT, B-LTA), and Adobe's
  `adbe-revocationInfoArchival` signed attribute, which 144 of the
  corpus's 593 signatures carry.
- **M12, a "Decide at kickoff" line:** are cms 0.3, x509-cert 0.3,
  x509-ocsp 0.3 and x509-tsp 0.2 published by then (D2)? p12-keystore on
  the published cms? Signing with PKCS#12 keys through ring or the rsa
  crate (RUSTSEC-2023-0071)? Trust anchors: user-trusted, the OS stores,
  the EU list (D13)? The facts behind them: 23% of the corpus's
  real-world signatures are BER, and the stable cms 0.2.3 reads none.
- **M12, `:411-415` (certificate encryption):** name the pieces: cms
  EnvelopedData; RSA key transport (PKCS#1 v1.5, OAEP) and ECDH (X9.63);
  des, rc2 and aes; decryption through the key-store trait; the 12 corpus
  files and pyHanko's test keys.
- **M13, `:418-420`:** add "shaping with harfrust (rustybuzz is
  archived), line breaks from icu_segmenter. Existing subsets rarely have
  the glyphs or the layout tables, so new text that needs them goes in a
  new font resource. Installed fonts are found through fontdb."
- **M14, `:434-435` (Compare):** add `similar` for text, with its
  deadline (imara-diff if a benchmark says); a pixelmatch-style kernel
  (`blazediff`, or a port); not `dssim` (AGPL-3.0); the report on
  sheaf's own writer.
- **M14, `:436-437` (Optimize):** add: re-subsetting keeps codes (unused
  glyphs are blanked in place; no crate subsets the bare CFF or Type 1
  programs PDFs embed); image work runs on M11's sample decoder; JPEG
  with `jpeg-encoder`, bilevel as G4, never lossy JBIG2; downsampling
  with `fast_image_resize` if it measures faster; Flate on zlib-rs, with
  any "maximum" setting (zopfli or libdeflate) decided by measurement.
- **M14, `:455-456` (PDF/A):** add the testing plan (the veraPDF CLI over
  `../pdf-corpus/conformance/`, a test tool and never linked; D3); the
  output intent (D14: the CC0 Compact-ICC-Profiles, or one generated by
  moxcms, already in the tree); PDF/A-1 conversion re-encoding LZW and
  JPX through M11's decoder.
- **M17, `:572-574`** ("Open: how to test it"): name what exists:
  `Window::debug_a11y_tree_json()` (gpui-pre 0.3.9 `window.rs:7011`),
  `accesskit_consumer` for walking a tree, `Element::a11y_synthetic_children`
  as the page view's hook. The open part becomes whether headless tests
  can activate the tree.
- **M17, `:581-584`** (Validate): add veraPDF's `ua1` and `ua2` flavours
  (1.30), and the Matterhorn split: 136 failure conditions, 89 that
  software can check and 47 that need a person. The review view exists
  for the 47.
- **Pending, `:817-837`** (Search across files): add to Open that a GUI
  index would be a plaintext copy of client text on disk (M11 proposal
  11), so an index needs its own decision on encryption and location.
  **Who:** when the entry leaves Pending.
- **Pending, `:850-854`** (Spell check): zspell is Apache-2.0 (its
  LICENSE file), last released 0.5.5 on 2024-06-13, last commit
  2024-07-09, suggestions behind an unstable feature: drop it. spellbook
  0.4.2 (2026-06-03): MPL-2.0 is file-level copyleft (offer its source);
  a few `unsafe` blocks. LibreOffice's en_US (SCOWL) is permissive.
  **Who:** when the entry leaves Pending (D15).

### `plan/m16-agent/README.md`: at M16's kickoff (#32 edits it now, so lines move)

- **`:94-95`, `:172`, `:175-176`, `:183`:** versions: ACP 3.5.0 and
  agent-client-protocol-rmcp 4.0.5 (both 2026-10-22); rmcp 3.7.0
  (2026-10-22).
- **`:200-202`:** the agent socket's uid check is tokio's
  `UnixStream::peer_cred`; std's is still unstable.
- **`:234`** (option 7, "Keeps rmcp and tokio out of the GUI"): tokio is
  already in sheaf-app's default build, through sheaf-plugin's
  wasmtime-wasi and wasmtime-wasi-http (tokio 1.53.2).
- **`:274-282`:** name the parts: tokio's or interprocess's named pipes
  (first instance, reject remote, security descriptor, client and server
  pid, impersonation), or uds_windows with `SIO_AF_UNIX_GETPEERPID`; the
  SID check is sheaf's own code on the windows crate (D16).
- **`:164`, `:200`, `:328`** (`sheaf-instance`): **done by #32,** whose
  agent.md already names instance-handoff there.

### `plan/complete/m10.5-edit-api/README.md`: at M14's kickoff

- **`:3667`:** after "Metric-compatible substitutes (Liberation)", add
  "Liberation covers Helvetica, Times and Courier; Symbol and
  ZapfDingbats need Foxit's substitutes (BSD-3, as PDFium and pdf.js
  ship them)" (D10).

### `plan/m05-scan/README.md` (the `m5-intake` branch): by the M5 work, on its branch

- **`:187-188` and `:621-624`:** add "re-checked 2026-10-23: `libsane-sys`
  doesn't exist, and no maintained SANE crate does either." The decision
  doesn't change.
- **`:282-293`:** a note under F16: "rxing in a plugin needs a
  wasm32-wasip2 build check; rxing-wasm targets wasm32-unknown-unknown."

### `README.md`: at the kickoffs named

- **`:365-366`:** "Scripting language for batch actions: Rhai … vs Lua"
  is listed as open, but `ROADMAP.md:466-468` settled it (plugins first,
  Rhai optional). Mark it settled; `:304`'s "(Lua or Rhai, not JS)"
  follows. **Who:** M15's kickoff.
- **`:358-363`** ("One parser or two? … Spike in M0"): answered (lopdf
  plus PDFium, then F13). Mark it settled, with a link to F13. **Who:**
  F13's triage.

### `vendor/lopdf/SHEAF.md` and `Cargo.toml`: with the weekend's upstream PRs (Greg, 2026-10-24/25)

- **`SHEAF.md:14`:** "No release has it yet" is stale: lopdf 0.46.0
  (2026-10-21) includes #598. Say so, and that f64 reals, `LazyPdf` and
  the caps are why the vendored copy stays.
- **`SHEAF.md:316-324`** ("Going back to crates.io"): note what upstream
  merged after the vendored commit: #603 (0.46.0, `2f497f8`); #613; #624,
  #625 and #630 (main, 2026-10-22/23). Compare each `sheaf:` hunk with
  upstream before the next move.
- **`Cargo.toml:106-113`:** the comment says the vendoring is because
  "lopdf 0.45.0 reads an earlier revision". Add that 0.46.0 fixes that,
  and that the patch now carries `LazyPdf`, f64 reals, caps and
  encryption.
- **`Cargo.toml` (B26's fix):** one workspace `flate2` entry with
  `zlib-rs` (D4).

### `plan/backlog/README.md`: in this PR

- **B5** (`:184-209`): the spike's result (entry 93).
- **F9** (`:665-686`): compare the `conformance/` tier with veraPDF's
  JSON; it skips without Java or Docker.
- **F13** (`:726-736`, "Upstream meanwhile"): what upstream has merged,
  and what's left to send (entry 2).
- **F13** (`:744-755`, "Maybe published"): the survey's result (entry 1).
- New entries B24–B29 and F18–F23 (next section).

### `docs/prior-art/rust-ecosystem.md`: in this PR

- **`:18`, `:19` and `:234-239`:** hayro (0.8.0; skrifa and hayro-cmap;
  `forbid(unsafe)` per crate, with SIMD dependencies; an oracle, its
  parts taken one by one), lopdf (vendored and patched; F13; eager
  `Document` verified), pdf-rs (0.10.0, f32, no AI-written PRs), krilla
  (0.8.2; PDF/A-1 to 4 and PDF/UA-1; a reference, not a writer for
  sheaf), typst-pdf (Apache-2.0, verified), pdfium-render (0.9.4); new
  rows for hayro-syntax, pdf_oxide, oxidize-pdf, qpdf-rs, pdf-writer
  (with hayro-write) and xmp-writer; a pointer to this memo.
- The fonts, security and image sections each proposed a verdict table
  of their own for that file. They stay here instead, linked from its
  introduction, so the verdicts live in one place.
