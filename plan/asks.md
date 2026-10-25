# Open questions for Greg

Every question still waiting on Greg, one line each, with the
recommendation where the plan gives one ("rec.") and a link (→) to where
it's argued. When one is ruled, record the ruling where it's argued and
delete its line here. Gathered 2026-10-25, when the plan was split. D1–D17
are [build or adopt](research/build-or-adopt/decisions.md)'s decisions.

## Between milestones and the order

- What comes after M11 is still open: M16, M10 slices 18–29 and the rest. [→](between/README.md)
- **D4:** one Flate backend for the workspace (B26). Rec.: zlib-rs explicitly. [→](research/build-or-adopt/decisions.md#d4-one-flate-backend-for-the-workspace)
- **D7:** an advisory and license check in CI (B27). Rec.: `cargo deny`. [→](research/build-or-adopt/decisions.md#d7-an-advisory-and-license-check-in-ci)

## M10: Forms

- **D1:** subsetter renumbers glyphs, against slice 7's glyph-ID rule. Rec.: subsetter behind a `/CIDToGIDMap` stream. [→](m10-forms/slices/s07-unicode-fallback-font.md)
- **D9:** how the field editor paints. Rec.: sheaf's own glyph runs through `paint_glyph`, never a PDF's font handed to the platform; spike first. [→](research/build-or-adopt/decisions.md#d9-the-field-editors-painting-m10-slice-11)
- Files wanted: Acrobat reference fills, rounding, XFA, a signed field, real practice forms. [→](m10-forms/decisions.md#asks-answered-at-kickoff-2026-10-15)

## M11: Security & redaction

- Proposals 5 and 7–16 are still to confirm at kickoff (6 was taken with slice 3). [→](m11-security/proposals/README.md)
- From the content interpreter (#50): [→](complete/m11-security/s03-content-interpreter.md#notes)
  - the interp gate budgets: assert them always, or keep them opt-in?
  - drawing charged to the page's budget: a separate cap instead?
  - FreeType's FTL credit: confirm, or rule the readers reimplemented from behavior?
  - the +15% / −11% interp rows after the review's fixes: bisect, or leave?
  - the text-bag gate row: a budget on it too?
  - the skrifa bump ([F26](backlog/F26.md)): with GPUI's next bump, or when F17 lands?
  - the hayro-cmap upstream PR ([F25](backlog/F25.md), public; an issue first).
- The standard 14's widths as exact for slice 5, and the tolerances bullet. Rec.: yes. [→](complete/m11-security/s03-content-interpreter.md#notes)
- **D5:** M11's image decoding (slice 6). Rec.: one sample decoder in sheaf-image; re-encode every image that shares JBIG2 globals. [→](research/build-or-adopt/decisions.md#d5-m11s-image-decoding-slice-6)
- **D6:** clipping line art. Rec.: i_overlay 9 directly; strokes expanded to outlines with kurbo. [→](research/build-or-adopt/decisions.md#d6-clipping-line-art-i_overlay-directly-and-strokes-m11-slice-6)
- **D11:** blank redacted glyphs' outlines in slice 5, not M14. Rec.: yes. [→](research/build-or-adopt/decisions.md#d11-blank-redacted-glyphs-outlines-in-m11-not-m14-m11-slice-5)
- **D12:** XMP. Rec.: keep a PDF/A file's claim with a minimal fresh packet; one quick-xml writer. [→](research/build-or-adopt/decisions.md#d12-xmp-one-writer-or-two-and-a-pdfa-files-claim-on-sanitize)
- Files wanted: Acrobat redaction and encryption files, a bank statement with a permissions password, a tagged Word PDF, and a Mac to check Preview. [→](m11-security/asks.md)
- Does sanitize remove field scripts, or whole fields (the edge to M10 slices 23–26)? [→](../ROADMAP.md#milestones)
- [B47](backlog/B47.md): keep refusing a form that draws itself, or draw it 40 deep as PDFium does?

## M12: Signatures

- **D2:** wait for RustCrypto's cms 0.3 round, or pin pre-releases? Rec.: don't hold; pin at kickoff. The roadmap's "PAdES waits for cms 0.3" reads as the other option. [→](research/build-or-adopt/decisions.md#d2-m12s-rustcrypto-line-wait-for-the-cms-03-round-or-pin-pre-releases)
- **D8:** the macOS keychain in M12. Rec.: yes. [→](research/build-or-adopt/decisions.md#d8-the-macos-keychain-in-m12)
- **D13:** ring for PKCS#12; trust anchors (user certificates and the OS stores); the HTTP client; rustls-webpki; a script font for typed signatures. [→](research/build-or-adopt/decisions.md#d13-m12s-other-kickoff-asks)
- Fill & Sign: where saved signatures are kept (a third kind of file, like spell check's word list). [→](m12-signatures/README.md#summary)
- Does signing create new signature fields (the edge to M10 slices 23–26)? [→](../ROADMAP.md#milestones)

## M14 and M17

- **D3:** veraPDF in tests and CI. Rec.: Java and the veraPDF CLI. Also: does sheaf validate other tools' files? [→](research/build-or-adopt/decisions.md#d3-verapdf-in-tests-and-ci-java-or-docker-m14-m17-f9)
- **D10:** M14's embed-all substitutes for Symbol and ZapfDingbats. Rec.: Foxit's. [→](research/build-or-adopt/decisions.md#d10-m14s-embed-all-substitutes-for-symbol-and-zapfdingbats)
- **D14:** the PDF/A output intent profile. Rec.: the CC0 Compact-ICC profiles. [→](research/build-or-adopt/decisions.md#d14-the-pdfa-output-intent-profile-m14)
- M14: how far to go on PDF/A and preflight (❓ in the roadmap). [→](m14-processing/README.md#summary)
- M17: how to test it [→](m17-accessibility/README.md#summary); the review as a panel or a mode [→](m17-accessibility/README.md#summary); which part first, and how far on PDF/UA [→](m17-accessibility/README.md#summary); whether it shares M14's validator [→](../ROADMAP.md#milestones).

## M16: Agent tools

- **D16:** M16 on Windows. Rec.: tokio named pipes with a SID check. [→](research/build-or-adopt/decisions.md#d16-m16-on-windows-bridge-threads-or-a-per-user-named-pipe-agentmd-decision-2)
- agent.md's kickoff items: the socket folder and how the relay finds it [→](m16-agent/transport.md#decisions-at-kickoff); fail when no sheaf runs (rec.) [→](m16-agent/transport.md#decisions-at-kickoff); localhost HTTP only if needed, off by default (rec.) [→](m16-agent/transport.md#decisions-at-kickoff); serve both protocol eras (rec.) [→](m16-agent/transport.md#decisions-at-kickoff); a snippet or `.mcpb` [→](m16-agent/transport.md#decisions-at-kickoff); how the approval card gets attention [→](m16-agent/transport.md#decisions-at-kickoff).
- Which ops a plan may hold, and whether "semantic edits only" still holds [→](m16-agent/README.md#summary); commands never run, writes never "always allow" [→](m16-agent/README.md#summary).
- The recommendations under the 2026-10-22 direction, not yet decided: handles, per-document consent (listing included), `open_document` and its approval card. [→](m16-agent/transport.md#every-open-document-and-opening-more-greg-2026-10-22)
- Still open in agent.md: Anthropic's answer on claude-agent-acp sign-in, limiting tools, MCP over ACP, threads, the §7216 wording and log, page-image caps. [→](m16-agent/agent.md#open)

## M5 (deferred)

- Asks 1–11, 13 and 14 were taken as recommended and stay open to review until the slice that depends on each starts. [→](m05-scan/README.md#where-it-stands)
- Ask 12's leftovers: which fi-6130 model and driver [→](m05-scan/asks.md); may a dev build run on a work PC [→](m05-scan/asks.md).

## Code signing and packaging

- The publisher (which may mean forming an LLC) [→](topics/code-signing.md#open-decisions); the provider [→](topics/code-signing.md#open-decisions); sign pdfium.dll (rec. yes) [→](topics/code-signing.md#open-decisions); automatic signing or a required reviewer [→](topics/code-signing.md#open-decisions).

## Plugins

- plugins/README.md: proposals 1–8 [→](topics/plugins/README.md#proposed-to-confirm); sheaf-edit's semver (rec.: non_exhaustive, an exact pin, semver-checks) [→](topics/plugins/README.md#open); helpers behind the WIT [→](topics/plugins/README.md#open); a Python HTTP helper [→](topics/plugins/README.md#open).
- cargo-sheaf-plugin.md: proposals 1–5 [→](topics/plugins/cargo-sheaf-plugin.md#proposed-to-confirm); a host component check (leaning yes) [→](topics/plugins/cargo-sheaf-plugin.md#open); `new --ci github` (leaning yes) [→](topics/plugins/cargo-sheaf-plugin.md#open).
- document-transforms.md: proposals 1–2 [→](topics/plugins/document-transforms.md#proposed-to-confirm); eleven open items, with recs (per-transform destinations, Rewrite input, refuse unreadable output, pause edits) [→](topics/plugins/document-transforms.md#open).
- plugin-image-writes.md: per-capability permissions [→](topics/plugins/image-writes.md#where-we-landed); the live handle's details [→](topics/plugins/image-writes.md#where-we-landed).

## Pending features and the backlog

- **D15:** spell check engines [→](research/build-or-adopt/decisions.md#d15-spell-check-engines-pending); where "Add to dictionary" writes, and in which milestone [→](pending.md#spell-check).
- Search across files, the GUI half: its entry point, and does it search unsaved changes? [→](pending.md#search-across-files)
- Placement: Markdown export's milestone [→](pending.md#export-to-markdown); following links on its own or with M13 [→](pending.md#follow-links-in-the-page-view); a milestone for the PDFlyer gaps? [→](pending.md#pdflyer-parity-gaps)
- File contention: read-only like Office, or view-only; the lock file's name; review in network folders, in or out. [→](topics/file-contention.md#when-we-come-back)
- [B4](backlog/B4.md): accept content reads on the UI thread?

## Worded as open, but ruled: fix the wording

Found while gathering these; the wording wasn't changed when the plan was
split.

- m5's "Ask 12 still open" (slice 2's notes), answered 2026-10-23. [→](complete/m05-scan/s01-intake-pipeline.md#notes)
- M11 slice 3's "lagging operators (For Greg)", ruled ("m11 perf sounds fine"). [→](complete/m11-security/s03-content-interpreter.md#notes)
- M10 slice 2's "For Greg: the other way is 0.45.0…": moot, the vendored lopdf shipped and 0.46.0 has the fix. [→](complete/m10-forms/s02-field-model-form-facts.md#notes)
- M16 asks how `sheaf mcp` reaches sheaf; Greg's direction (option 1) is recorded in agent.md. [→](m16-agent/README.md#summary)
- agent.md's "Placement: after M15, or before" against the roadmap's "after M16, needn't wait for M15". [→](m16-agent/agent.md#open)
- Plugin plans timed against M10.5, which is done: "settle by M10.5 slice 11", the rename "in slice 20", "API 0.4 planned", "slices 1–3 between M10.5 slices". [→](topics/plugins/README.md#open)
- The plugin queue's "nothing below is built yet", and its open items for duplicate pages and regions, both built. [→](topics/plugins/queue.md#4-find-duplicate-pages-duplicate-pages--done-2026-10-13)
- m5's proposals and asks headings still say "to confirm at kickoff"; the kickoff was 2026-10-23. [→](m05-scan/proposals/README.md)
