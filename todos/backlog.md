# Backlog: bugs and features found along the way

Status: **open log** (started 2026-10-21). Bugs and feature ideas that
turn up during milestone work, written down so they aren't lost. Nothing
here is scheduled. We don't fix these in the middle of a milestone
phase; they're triaged between phases.

## How to use it

- **Add, don't fix.** When something comes up that isn't part of the
  current slice, write an entry and keep going. Agents working a slice
  do the same.
- **One entry per item**, newest at the bottom of its section. IDs are
  never reused: `B1`, `B2`… for bugs, `F1`, `F2`… for features.
- **Check for overlap first.** A feature already in `ROADMAP.md` (a
  milestone, the parity checklist, Pending or Not planned) or in a
  `todos/` memo gets a line there, or a link here, not a duplicate.
- **Triage between phases.** Each entry moves to [Closed](#closed),
  saying where it went:
  - fixed, with the commit or PR;
  - a slice in a milestone plan;
  - a feature into `ROADMAP.md` Pending, where every feature proposal
    starts;
  - its own memo in `todos/`, if it needs research;
  - won't fix or not planned, with a reason.

### Bug entry

```markdown
### B<n>: <short title>

- **Found:** <date>, <who>, during <milestone/slice>
- **Where:** <crate, file:line, command or view>
- **Severity:** data loss | wrong output | crash | annoyance
- **What happens:** <observed>
- **Expected:** <what should happen>
- **Repro:** <steps, corpus file, or test>
- **Notes:** <suspected cause, related entries or memos>
```

### Feature entry

```markdown
### F<n>: <short title>

- **Found:** <date>, <who>, during <milestone/slice>
- **What:** <the feature>
- **Why:** <who needs it, what it unblocks>
- **Overlap:** <roadmap items or memos it touches, or none found>
- **Notes:** <prior art, rough size, open questions>
```

## Bugs

### B2: Scrolling over a dialog also scrolls the page behind it

- **Found:** 2026-10-21, Greg, in the command palette, during M10.5
- **Where:** `sheaf_ui::dialog::backdrop` (`crates/sheaf-ui/src/dialog.rs`)
- **Severity:** annoyance
- **What happens:** with the command palette open, the scroll wheel
  scrolls the palette's list and the page view underneath at the same
  time.
- **Expected:** a modal dialog takes the wheel. Only the dialog
  scrolls, or nothing does.
- **Repro:** open a document long enough to scroll, open the palette
  (Ctrl+Shift+K), and scroll over its list.
- **Notes:**
  - **Cause, from reading the code (not yet reproduced or tested):** the
    backdrop is a full-window `absolute().inset_0()` div with a
    mouse-down listener, but no `.occlude()`. In GPUI a scroll handler
    runs when its hitbox `should_handle_scroll`, and only a hitbox in
    front set to block the mouse (`occlude()`) stops that.
    `block_mouse_except_scroll` lets scrolling through on purpose
    (`gpui-pre` 0.3.9 `elements/div.rs`, `window.rs`). So the page
    view's `.on_scroll_wheel(… on_wheel …)` and its `overflow_scroll`
    both still fire, and so does any panel under the cursor.
  - **The fix is probably one line:** `.occlude()` on the backdrop. The
    palette's list draws above its own backdrop, so it keeps scrolling.
  - **It's every dialog, not just the palette:** 20 dialogs use the
    backdrop (export, print, OCR, scan, combine, split, insert and
    replace, crop, watermark, header/footer, labels, blanks, properties,
    Set Status, bookmark tools, toolbar, plugin manager, plugin forms).
    Overlays that don't use it (menus, popovers, context menus) mostly
    call `.occlude()` already; check the rest.
  - **Test:** a headless GPUI test that opens the palette, sends a
    scroll-wheel event over it, and checks that the page view's scroll
    offset hasn't changed. GPUI tests are Linux-only, so the helpers
    need `cfg(all(test, target_os = "linux"))`.

### B3: Properties says "Fast Web View: Yes" after an incremental save

- **Found:** 2026-10-22, Claude, while Greg and Claude discussed where
  linearization belongs (M14), during M10.5 slice 18
- **Where:** the vendored lopdf's `vendor/lopdf/src/lazy.rs:443`, which
  sets `XrefFacts.linearized`; `crates/sheaf-doc/src/materialize/append.rs:477`,
  which carries it into the output record; shown by
  `crates/sheaf-app/src/properties_dialog.rs:147`
- **Severity:** wrong output
- **What happens:** a file counts as linearized whenever `/Linearized`
  appears within 1,024 bytes of its header. An Append keeps the input's
  flag, and a reopen finds the same dictionary at the top. So after
  sheaf (or any tool) appends an update to a linearized file,
  Properties still says Fast Web View: Yes. The dictionary's `/L` and
  hint tables no longer describe the file, and readers drop the fast
  path.
- **Expected:** Yes only when the linearization dictionary still holds.
  At least `/L` must equal the file's length (ISO 32000-1 Annex F: an
  updated file whose `/L` doesn't match isn't treated as linearized).
  After an Append, the output record says not linearized.
- **Repro:** found by reading the code, not yet run. Take a linearized
  corpus file (the pool sample has 216), add a mark, save (an Append),
  and open Properties. `qpdf --check-linearization` on the saved file
  should disagree with sheaf.
- **Notes:**
  - The fix is small: compare `/L` with the file length where the
    flag is set, and write `false` in the Append's record.
  - realize_corpus groups its stats by this flag, so its "linearized"
    row includes files updated after linearizing.
  - Writing linearized files is M14's (Optimize: linearization as a
    writer option).

### B4: The realizer's predicate and the emission disagree on a page that can't be read

- **Found:** 2026-10-22, Claude, in PR #26's pre-merge run
  (`realize_corpus --ignored`), during M10.5 slice 18. Greg merged #26
  with it logged here.
- **Where:** `crates/sheaf-doc/src/realize/untouched.rs`
  (`untouched`); `crates/sheaf-doc/src/materialize/emit/content.rs`
  (`stack`)
- **Severity:** annoyance (extra work; the pixels are right)
- **What happens:** `the_predicate_agrees_with_the_emission_on_every_page`
  fails on `pool/safedocs/0874/0874785.pdf` page 0, under all four
  inputs (screen, plugin, print, print without markup): "the model says
  false, the emission true", 4 of 242,055 checks.
  - The tier's edit puts a header ("Realized <<1>> of <<N>>", with a
    Bates slot) and an over-watermark on the page.
  - Page 0's content can't be read: stream 49 doesn't inflate ("invalid
    distance too far back"). That's real damage, not a decryption bug;
    qpdf and poppler fail on it too.
  - The emitter can't read the page's blocks, so it writes the page as
    its base with the Note "No … header or footer is drawn on page 1,
    whose content can't be read." To the emitter, the page is untouched.
  - The model never reads content. It sees generators on an unmarked
    page and says the page changed.
- **Expected:** the two agree. The model should call such a page
  untouched, so it draws from the base engine.
- **Repro:** `SHEAF_REALIZE_ONLY=pool/safedocs/0874/0874785.pdf cargo
  test -p sheaf-ops --test realize_corpus -- --ignored
  the_predicate_agrees_with_the_emission_on_every_page`
- **Notes:**
  - **Only conservative:** the page goes to its group's overlay, which
    writes it exactly as the base, so the screen never shows a stale
    page. The pixel tier passes on it.
  - **Since slice 17 (`bf7baf2`):** before, the legacy header installer
    wrapped the content without reading it, so the emission changed the
    page too (0 of 20 at `fa14236`, 4 of 20 at `bf7baf2`). The same gap
    has been there for a watermark alone since slice 16, hidden by the
    header.
  - **A proposed fix, about 60 lines in 4 files** (an investigation's;
    it passes the narrowed predicate and pixel tiers and the sheaf-doc
    lib):
    - `untouched` becomes `boxes_kept && annots_kept && (!(drawn_over
      || generated) || unread(def))`.
    - `unread(def)` is `(no markers || filters non-empty) &&
      !src.readable(index)`. That's exactly where `stack` decides by
      the page's blocks.
    - A new `Source::readable(index)` caches the answer either way,
      because `blocks()` doesn't cache a failure.
    - A test, `a_page_whose_content_cant_be_read_is_untouched_under_a_header`,
      builds an unclosed string beside a readable page and expects
      `[true, false]`.
  - **Its cost, for a decision:** the predicate would read a page's
    content, once per source page, on the UI thread (`view()`, and
    `find()` over up to 128 pages of a group the first time it's seen
    after a header is added). It's the same read the worker's delta does
    and they share the cache, so the total work doesn't change, but some
    of it moves to the UI thread. No fix keeps the predicate free of
    content reads and the tier strict: generator commits read no content
    (the Bates gate depends on it), and drawing over unreadable content
    isn't safe.
  - Render keys still include the generators for such a page: extra
    re-renders, never wrong pixels.

### B9: A radio group keeps its value when its chosen button's page is removed

- **Found:** 2026-10-22, M10.5 slice 19's export corpus tier
  (pre-existing: `DeletePages`')
- **Where:** `DeletePages`' lowering (`crates/sheaf-doc/src/tx/`), and so
  an extract's `KeepPages`
- **Severity:** wrong output
- **What happens:** delete the page holding a radio group's chosen
  button (or extract the other pages) and the group keeps its `/V`,
  which no button left can show. `forms::check` then reports
  `NoAppearance`, and `StaleSigFlags` on the delete.
- **Expected:** what's left is consistent: the group's `/V` is a state
  one of its remaining buttons has, or `/Off`; the form check finds
  nothing new.
- **Repro:** `forms/safedocs/0070616.pdf`, field `Gruppieren4`: the
  export tier's `export_round_trips` asserts this as its one known
  failure.
- **Notes:** the same question for a check box group and for a
  choice's `/Opt`-bound value; worth checking what Acrobat does to `/V`
  on Delete Pages.

### B10: After a Combine, the "XFA part holds values" bar can stay up

- **Found:** 2026-10-22, M10.5 slice 19 (step 7, Combine's hand-off)
- **Where:** the app's Combine hand-off (`crates/sheaf-app/`), the
  adopted window's form facts
- **Severity:** annoyance
- **What happens:** Combine is one transaction on the first file's
  session, handed to a window as made, answered with `PRINT_ANSWERS`
  (which drop an XFA part's values). The adopted window keeps the first
  file's form facts as opened, and `xfa_values_dropped` isn't set. So a
  hybrid first file whose XFA held values can still show the bar after
  the combine dropped them.
- **Expected:** the bar reflects the combined document: gone, or saying
  the values were dropped.
- **Repro:** Combine with a hybrid XFA form holding values as the first
  file.
- **Notes:** none found in the corpus runs; read from the code.

### B19: A bookmark selection isn't tracked, so a re-lowered edit can land on other pages

- **Found:** 2026-10-23, M10.5 slice 20
- **Where:** `PageSel::Bookmark`'s resolve (`crates/sheaf-doc/src/tx/`),
  which records only `Key::Order`, not the outline `bookmark_span` reads
- **Severity:** wrong output
- **What happens:** an edit that selects pages by a bookmark, re-lowered
  (`Stale::Relower`) after another edit moved that bookmark, resolves to
  the bookmark's new span with no conflict. Only a transaction that was
  handed out conflicts, and its Conflict names no edit. Decision reads
  of a bookmark selection have the same gap.
- **Expected:** a move of the bookmark (or of the next one, which ends
  its span) between the read and the commit conflicts, as other stale
  reads do.
- **Repro:** a plugin's edit on `{"bookmark": "Schedule C"}` while the
  user drags that bookmark to another page; the call ends and
  re-lowers.
- **Notes:** record the outline keys the span reads (the bookmark and
  its next sibling at each level).

### B20: A plugin's region events wait forever after a window closes mid-call

- **Found:** 2026-10-23, M10.5 slice 20 (pre-existing)
- **Where:** the app's plugin call queue (`crates/sheaf-app/src/plugins.rs`),
  the `regions` key
- **Severity:** annoyance (a stuck plugin)
- **What happens:** if a region call is in flight when its window
  closes, `ws.update_in` fails and the plugin's `regions` key stays
  busy, so its later region events in every window never run. Changes
  are keyed per view and aren't affected.
- **Expected:** a closed window ends its calls and frees the key.
- **Repro:** a plugin that handles regions slowly; select a region,
  close the window before it returns, then select a region in another
  window.

### B30: Images to PDF and scan outputs have no /Producer or /CreationDate

- **Found:** 2026-10-23, the backlog-fixes PR (beside B8)
- **Where:** sheaf-ops `images.rs` and `scan.rs`, whose sessions start
  from `Session::new_empty`; File > Create > From Images and From Scans,
  `sheaf images-to-pdf` and `sheaf scan`
- **Severity:** annoyance
- **What happens:** the output takes its `/Info` from its empty primary,
  which has none, so it has no `/Producer` or `/CreationDate`. Combine
  keeps its first file's `/Info`, which is right.
- **Expected:** `/Producer` sheaf and `/CreationDate` when it was made, as
  the comment summary has again (B8).
- **Repro:** `sheaf images-to-pdf a.png -o out.pdf`, then
  `qpdf --show-object=trailer out.pdf`: `<< /Root 3 0 R /Size 7 >>`.
- **Notes:** M5's intake rework (`m5-intake`) owns scan; leave scan.rs to
  it. B8's `Session::new_titled`, an empty primary with an `/Info`, is
  the likely fix.

### B23: A fuzz test's wall-clock budget fails when the machine is busy

- **Found:** 2026-10-23, the instance-handoff migration's workspace run
  (pre-existing)
- **Where:** `source::fuzz::tests::fuzz_bodies_survive_mutated_fixtures`
  (`crates/sheaf-doc/src/source/fuzz.rs:1394`), `took < 60 s`
- **Severity:** annoyance (a flaky test)
- **What happens:** at load average ~60 it took 70.9 s and failed; run
  alone it passes in 6.25 s. A slow CI runner could trip it too.
- **Expected:** the test bounds the work, not the wall clock: a cap on
  steps or bytes per mutated body (what it really guards against is
  unbounded work), or CPU time rather than elapsed time.
- **Notes:** the 7k gates measure time on a quiet machine on purpose;
  a unit test in the default suite shouldn't depend on load.

### B24: G4 encoding truncates images wider than 65,535 pixels

- **Found:** 2026-10-23, the build-or-adopt audit
- **Where:** `sheaf_image::encode::g4`
  (`crates/sheaf-image/src/encode.rs:128-143`), behind images-to-pdf,
  Combine, image inserts, the intake's G4 pages and TIFF export
- **Severity:** wrong output
- **What happens:** `enc.encode_line(pels, w as u16)` (`:136`) encodes
  each row at the width modulo 65,536, while `/Columns` (or the TIFF's
  `ImageWidth`) keeps the true width, so the image decodes as garbage. A
  600 dpi scan over 109 in wide is enough. Nothing in `sheaf_image::io`
  caps width.
- **Expected:** the image is encoded at its width, or refused with a
  message.
- **Repro:** a black-and-white PNG 70,000 × 100 px through
  `sheaf images-to-pdf`.
- **Notes:** `fax` 0.3.0 (2026-07-13) takes `u32` widths: bump it, or
  refuse widths it can't take. The test helper's `decode_g4(…, w as u16,
  …)` (`:160-161`) has the same cast. [build-or-adopt.md](build-or-adopt.md), entry 38.

### B25: Fax TIFFs (CCITT Group 3, Modified Huffman) are refused

- **Found:** 2026-10-23, the build-or-adopt audit
- **Where:** `sheaf_image::io` (TIFF reading), through tiff 0.11.3
- **Severity:** annoyance (input refused)
- **What happens:** tiff 0.11.3 decodes only `Fax4` among the fax
  compressions. `Fax3` (3) and `Huffman` (2) fail with
  `UnsupportedCompressionMethod`
  (`tiff-0.11.3/src/decoder/image.rs:604-620`), and `io.rs` has no
  fallback. Fax software's TIFFs (Class F) are G3.
- **Expected:** they open, as G4 TIFFs do.
- **Repro:** a G3 TIFF from a fax service in `sheaf images-to-pdf` (not
  reproduced; found by reading the decoder).
- **Notes:** decode the strips with `hayro-ccitt` (G3 1D and 2D, with
  T4Options' EOL and byte-align flags), which M11's image decoder also
  wants. B15 (on the `m5-intake` branch) lists the other TIFF kinds
  sheaf refuses; triage them together. tiff decodes planar data itself
  (`decoder/mod.rs:1147-1222`), so B15's planar case is sheaf's own
  refusal. [build-or-adopt.md](build-or-adopt.md), entry 16.

### B26: The Flate backend depends on which crates are in the build

- **Found:** 2026-10-23, the build-or-adopt audit
- **Where:** `flate2` feature unification: `crates/sheaf-app/Cargo.toml:38`
  (`zip` with `deflate-flate2-zlib-rs`) against `flate2 = "1"` everywhere
  else
- **Severity:** annoyance (output differs between builds)
- **What happens:** the shipped `sheaf` binary compresses with zlib-rs.
  The `sheaf-cli` binary, per-crate test runs (`cargo test -p
  sheaf-doc`) and builds without the plugins feature use miniz_oxide.
  On the same data the two give different bytes and sizes at levels 1, 6
  and 9 (measured with flate2 1.1.10; at level 1, 435,764 bytes against
  643,987). So a test that pins compressed bytes or sizes passes or
  fails by how it's run.
- **Expected:** one backend, named once in the workspace, in every
  build.
- **Repro:** compress the same buffer with flate2 built with default
  features and with `zlib-rs`, and compare.
- **Notes:** memory sources rely on "the same inputs make the same
  bytes" (`sheaf-doc/src/memory.rs:5-6`); check whether anything replays
  one across builds or versions. The audit recommends zlib-rs, which
  ships today ([build-or-adopt.md](build-or-adopt.md), D4).

### B27: Nothing checks dependencies for advisories or licenses

- **Found:** 2026-10-23, the build-or-adopt audit
- **Where:** CI (`.github/workflows/`); there's no `deny.toml`
- **Severity:** annoyance (a missing check)
- **What happens:** no advisory or license check runs. The audit found
  by hand that wasmtime 49.0.2 sat behind a critical sandbox-escape
  advisory (GHSA-mghf-wjfp-p5xm; PR #33 bumps it), and checked the
  dependency licenses it met (BSL-1.0, ISC, MPL-2.0, GPL) one by one.
  M12 brings crypto crates with live advisories (`rsa`:
  RUSTSEC-2023-0071, unpatched).
- **Expected:** CI runs `cargo deny check advisories licenses` (or
  `cargo audit`) with a reviewed ignore list, so a new advisory or a
  license change fails a build.
- **Repro:** `ls deny.toml`; `rg -i 'deny|audit' .github/workflows/`.
- **Notes:** small. Start with licenses and advisories; bans later.
  Overlap: `todos/code-signing.md` (release hygiene). The license check
  covers crates, not data files (F18). [build-or-adopt.md](build-or-adopt.md), D7.

### B28: Printing may not work in the Flatpak build

- **Found:** 2026-10-23, the build-or-adopt audit
- **Where:** sheaf-print's CUPS path (`crates/sheaf-print/src/lib.rs:5-9`;
  `todos/m7.md:23-37`), in the Flatpak
- **Severity:** annoyance (unverified: printing may fail outright)
- **What happens:** M7 prints through CUPS's command-line tools
  (`lpstat`, `lpoptions`, `lp -`) and chose them over the print portal
  because the portal is "sandbox-only" (`todos/m7.md:29-30`). But the
  Flatpak build is sandboxed: inside it, `lp` must be in the runtime or
  bundled, and the app needs `--socket=cups` (Flatpak 1.6 and later).
  Whether org.freedesktop.Platform includes `lp` is unverified.
- **Expected:** printing works in the Flatpak.
- **Repro:** not run: print from the Flatpak build.
- **Notes:** bundle the CUPS client tools with `--socket=cups`, or use
  the print portal in the Flatpak only, if bundling is awkward. Overlap:
  packaging (`todos/installer.md`), M7. Sources:
  [flatpak-run](https://www.mankier.com/1/flatpak-run),
  [KDE's `--socket=cups`](https://phabricator.kde.org/R256:2415ebe0b7ed42a2eed468c9da10d3924dd9b1c1).
  [build-or-adopt.md](build-or-adopt.md), entry 95.

### B29: Search misses ligatures, compatibility forms and NFD text

- **Found:** 2026-10-23, the build-or-adopt audit
- **Where:** Find, Patterns and `sheaf search`:
  `crates/sheaf-doc/src/page_chars.rs:68-80` (`Query::literal`,
  case-insensitive by the regex crate's simple case folding)
- **Severity:** wrong output (a missed match)
- **What happens:** neither the page text nor the query is normalized.
  So "file" may not match text drawn with a ﬁ ligature (U+FB01, from a
  ToUnicode or OCR), full-width digits in an EIN don't match digits, and
  an NFD query (macOS input) doesn't match NFC text, or the reverse.
  Whether PDFium's text page already expands U+FB01 is unverified.
- **Expected:** they match, and match offsets map back to characters.
- **Repro:** not run: a fixture with a ﬁ ligature, with and without a
  ToUnicode; search "file".
- **Notes:** born-digital statements and OCR layers carry ligatures and
  compatibility forms, so a missed EIN or name is a missed redaction or
  a wrong answer from the agent. Normalize the page text and the query
  (NFKC, or a narrower ligature map) and map offsets back; `PageChars`
  already maps byte offsets to characters, and unicode-normalization is
  already a dependency. Full case folding (ß/ss) would need `caseless` or
  `icu_casemap`. Overlap: M10.5 slice 19's `Query::literal`; Pending
  "Search across files"; M11 search and redact, whose marks must still
  land on the right glyphs. [build-or-adopt.md](build-or-adopt.md), entry 85.

### B31: sheaf and pyHanko count a file's signatures differently

- **Found:** 2026-10-23, the B5 PR's tier over `signatures/`
  (`the_corpus_s_signatures_agree_with_pyhanko`, which tolerates it as
  "counted otherwise")
- **Where:** `forms::sigs::read` (which signature fields sheaf
  enumerates), against `signatures/manifest.tsv`'s count
- **Severity:** unknown until it's understood
- **What happens:** in 5 files the counts differ: 0055607 (sheaf 2,
  pyHanko 22), 0064423 (2 vs 6), 0053329 and 0055217 (1 vs 2), 0014092
  (2 vs 1). Each file's latest revision holds sheaf's number of
  signature fields, so the difference is in which fields are
  enumerated (earlier revisions, or kids sharing one `/V`), not in the
  digests.
- **Expected:** sheaf's count is the one a save's ask should give. If
  pyHanko is counting signatures from earlier revisions that a later
  one dropped from the form, a Rewrite breaks those too, and the ask
  should say so.
- **Repro:** `SHEAF_SIGS_ONLY=signatures/safedocs/0055607.pdf cargo
  test -p sheaf-doc --lib the_corpus_s_signatures_agree_with_pyhanko --
  --nocapture` (all 5 under `signatures/safedocs/`).
- **Notes:** `qpdf --json` on each shows its fields by revision.

### B14: A TIFF's Orientation tag is ignored by images-to-pdf, Combine and image inserts

- **Found:** 2026-10-23, M5 slice 1
- **Where:** `sheaf_image::io::tiff_pages`
  (`crates/sheaf-image/src/io.rs`), behind `sheaf images-to-pdf`,
  Combine's images and `PageSource::Images`
- **Severity:** wrong output
- **What happens:** a TIFF whose Orientation tag says the image is
  turned or mirrored comes out unturned. `io::load`, `decoded` and slice
  1's `frames` (what `sheaf scan` reads) do turn it, so the same file
  looks different scanned and inserted.
- **Expected:** every reader honors the tag, as `load` does.
- **Repro:** a TIFF with Orientation 6 (turned 90°); `sheaf
  images-to-pdf` it, and `sheaf scan` it; compare.
- **Notes:** `tiff_pages` must keep its output byte for byte otherwise
  (its CMYK conversion is pinned); apply the tag after decoding, as
  `frames` does for later pages.

### B15: Some TIFF kinds are refused: CMYK16, float RGB/RGBA, planar

- **Found:** 2026-10-23, M5 slice 1
- **Where:** `sheaf_image::io::tiff_image`, behind images-to-pdf,
  Combine's images, `PageSource::Images`, and a scan's TIFF pages after
  the first
- **Severity:** annoyance
- **What happens:** these kinds fail with an error. The image crate's
  `load` reads them, so `sheaf scan` reads such a TIFF's first page (it
  uses `load` for it) but refuses its later pages.
- **Expected:** read them, or say which kind isn't supported.
- **Repro:** a planar RGB TIFF in `sheaf images-to-pdf`.
- **Notes:** rare from scanners and fax software. Two-sample gray
  (tiff 0.11's `Multiband`) is refused by both paths; `tiff_image`'s
  `GrayA` arms are unreachable.

### B16: The command line hangs if its work panics

- **Found:** 2026-10-23, M5 slice 1's review
- **Where:** `sheaf-cli`'s `progress::run`
- **Severity:** crash (shows as a hang)
- **What happens:** a panic in the work unwinds before
  `done.store(true)`, so `thread::scope` waits forever for the progress
  drawing thread, at 0% CPU. A debug build's overflow on a huge TIFF
  header found it (that overflow is fixed).
- **Expected:** the panic ends the command with its message and a
  failing exit code.
- **Repro:** any subcommand whose work panics, run through
  `progress::run`.
- **Notes:** set `done` from a drop guard.

### B17: The Combine dialog decodes every image just to count its pages

- **Found:** 2026-10-23, M5 slice 1
- **Where:** `combine_dialog.rs` (the background `inspect`, around line
  119)
- **Severity:** annoyance (slow)
- **What happens:** to show each item's page count, it decodes every
  image in full, and since slice 1 G4-encodes the black and white ones
  too. A folder of large scans takes a long time to list.
- **Expected:** count a TIFF's images from its headers (slice 1's
  `count_frames`) and treat other images as one page, without decoding.

### B18: images-to-pdf holds every gray and color page's pixels until it writes

- **Found:** 2026-10-23, M5 slice 1
- **Where:** `sheaf_ops::images::images_to_pdf` (`read` collects a
  `Vec<PageImage>`)
- **Severity:** annoyance (memory)
- **What happens:** every image is read before anything is written.
  Black and white pages are G4 bytes from slice 1 on, but gray and color
  pages (a PNG's pixels) are held raw: 100 color Letter pages at 300 dpi
  are about 2.5 GB.
- **Expected:** a page at a time, as `sheaf scan` now runs (M5's
  intake), or each page encoded as it's read.
- **Notes:** images-to-pdf doesn't clean pages, so it could run on the
  intake with no cleanup step.

### B32: An untitled document's recovery sidecar is never encrypted

- **Found:** 2026-10-23, M5 slice 2
- **Where:** `sheaf_doc::recovery::write_untitled`, the app's
  `DocView::write_recovery` (untitled arm)
- **Severity:** privacy
- **What happens:** a document made in memory has no encryption, so its
  sidecar in `state_dir()/recovery/` is written in the clear. Pages
  inserted into an untitled document from an encrypted PDF then sit
  unencrypted in the state folder until it's saved or closed.
- **Expected:** what a Save As would write anyway, so arguably fine. But
  a user who opened an encrypted file may not expect its pages to
  persist in the clear while a document has no file.
- **Notes:** options: carry the source's encryption into the untitled
  document's sidecar, or say so when pages from an encrypted file land
  in one. Overlap: M10.5 §11.10 (recovery).

### B33: Opening a file in the recovery folder can delete an untitled sidecar

- **Found:** 2026-10-23, M5 slice 2
- **Where:** `sheaf_doc::recovery::find` (`crates/sheaf-doc/src/recovery.rs:313`)
- **Severity:** annoyance (an extreme edge case)
- **What happens:** `find` beside a file removes an untitled sidecar as
  malformed. So opening a file named `state_dir()/recovery/Untitled scan
  1` (no extension) deletes that document's crash recovery.
- **Expected:** `find` beside a file ignores an untitled sidecar rather
  than removing it.
- **Notes:** tiny; nobody opens files in the state folder on purpose.

### B34: A file's recovery sidecar can outlive an undo to its saved version

- **Found:** 2026-10-23, M5 slice 2 (pre-existing)
- **Where:** `DocView::write_recovery`'s file arm
  (`crates/sheaf-app/src/docview.rs:1537`) and its completion
- **Severity:** annoyance
- **What happens:** the clean-document discard runs before the
  `sidecar_writing` check, and the completion keeps a sidecar while its
  base is unchanged. A write that takes over 3 s and lands after the
  document was undone to its saved version, or closed clean, stays. The
  next open then offers to recover undone edits.
- **Expected:** a write that completes on a clean document removes its
  sidecar, as the untitled arm (slice 2) already does.
- **Repro:** a large file, an edit, then an undo while its first sidecar
  write is still running; close; reopen.
- **Notes:** slice 2's untitled arm checks dirtiness when a write
  completes. Slice 7 (Insert from Scanner into an open file) reworks the
  file arm's writes, so it may be the place to fix it.

### B35: Opening a file reads its whole recovery sidecar and holds it until answered

- **Found:** 2026-10-23, M5 slice 2 (pre-existing)
- **Where:** `sheaf_doc::recovery::find`
- **Severity:** annoyance (memory)
- **What happens:** `find` beside a file reads the whole sidecar at open
  and holds it until the Recover / Discard prompt is answered. A large
  Full sidecar (a Rewrite of a big file) costs its size in memory for as
  long as the prompt is up.
- **Expected:** read the head to offer, and the body on Recover, as
  slice 2's `find_untitled` and `restore_untitled` now do.

### B36: sheaf-doc's tests leave scratch folders in /tmp on every run

- **Found:** 2026-10-23, M5 slice 2 (pre-existing)
- **Where:** `crates/sheaf-doc/src/model.rs:4119` (`sheaf-model-<pid>-<n>`)
  and `materialize/rewrite_tests.rs:2886` (`sheaf-save-…`)
- **Severity:** annoyance (tests)
- **What happens:** neither removes its folder, so they pile up: 7,041
  `sheaf-model-*` and 38,367 `sheaf-*` entries at slice 2's wrap-up. On a
  machine whose /tmp is RAM (tmpfs), that's memory.
- **Expected:** a temp dir that removes itself on drop, or cleanup at
  the end of each test. (sheaf-doc has no `tempfile` dev-dependency
  today.)

### B37: A stray doc comment above `open_recent` in workspace.rs

- **Found:** 2026-10-23, M5 slice 2 (pre-existing)
- **Where:** `crates/sheaf-app/src/workspace.rs:1172`
- **Severity:** cosmetic
- **What happens:** `/// Document > Recognize Text (OCR)…: open the
  options card…` sits above `open_recent`'s own doc comment, so rustdoc
  shows it on the wrong function.
- **Expected:** it moves to the OCR action it describes, or goes.

### B38: Corpus manifests are read without their CSV quoting

- **Found:** 2026-10-23, between milestones, while building a fetcher
  for the corpus
- **Where:** `crates/sheaf-doc/src/forms/corpus.rs:189`
  (`parse_manifest`)
- **Severity:** wrong output (tests)
- **What happens:** the corpus's tools write its manifests with
  Python's `csv` module, which quotes any field holding a tab, a newline
  or a `"`. `parse_manifest` splits rows at newlines and cells at tabs,
  and never unquotes.
  - In `signatures/manifest.tsv`, the `signatures_message` of
    `signatures/safedocs/0059407.pdf` holds a newline. Its row is cut
    short, losing its `bucket`, and the rest becomes a row of its own
    whose `path` is `while parsing asn1crypto.cms.ContentInfo"`.
    `Corpus::signatures()` has 532 entries for 531 files.
  - Four `note` cells keep their quotes, with the inner ones doubled:
    three `conformance/bfo/` files, and
    `parsing/pdfa-safedocs/Miscellaneous Targeted Test PDFs/Dual-startxref.pdf`.
- **Expected:** rows read as Python wrote them: quoted fields unquoted,
  a newline inside quotes kept in its field.
- **Repro:** count `Corpus::signatures()`, or look for the entry above.
- **Notes:** no test fails today. The stray row has no
  `signatures_read`, so the integrity tier filters it out, and no test
  reads `note`. Any new reader of these columns would see it.

### B39: imageproc's Otsu threshold overflows on large one-gray images

- **Found:** 2026-10-23, M5 slice 3 (pre-existing)
- **Where:** imageproc 0.27's `contrast::otsu_level`, called by
  `sheaf_image::clean` (`crates/sheaf-image/src/clean.rs:289` and `:466`)
- **Severity:** wrong output (a panic in debug builds)
- **What happens:** `otsu_level` multiplies in `u32`
  (`threshold as u32 * hist_count`, and `width * height`). On an image
  with more than about 16.8 million pixels of one gray level it panics
  in debug builds and gives a wrong threshold in release. A blank 600 dpi
  Legal page is 42.8 million pixels.
- **Expected:** a threshold computed in `u64`, as slice 3's patch reader
  now does (`codes/patch.rs`'s own `otsu_level`).
- **Notes:** step 3 of slice 3 saw clean.rs call it on reduced images
  only, but didn't trace every caller. Either use the patch reader's
  `u64` version in clean.rs too, or check each call's image size. The
  bug is upstream's: worth an issue or a PR to imageproc.

### B40: The Arlington model gives AnnotSquare `/CA` from PDF 2.0

- **Found:** 2026-10-24, the Arlington checks (F22)
- **Where:** upstream's `tsv/latest/AnnotSquare.tsv`
  (vendor/arlington-pdf-model); its `tsv/1.7` AnnotSquare has no CA
- **Severity:** annoyance (an error in the model)
- **What happens:** a Square mark's `/CA` in a file below 2.0 is a
  `too-new` finding.
- **Expected:** `/CA` from 1.4. ISO 32000-1 Table 170 gives every markup
  annotation `/CA` from 1.4, Table 169 makes Square one, and AnnotCircle,
  Square's twin in Table 177, has it from 1.4.
- **Repro:** `every_mark_kind_holds` without the allowance row
- **Notes:** held by `AnnotSquare CA too-new` in
  `crates/sheaf-ops/tests/arlington_allowed.tsv`. Worth reporting to
  pdf-association/arlington-pdf-model, with B45; a public report, so
  Greg approves it first.

### B41: A connector is a Link annotation with every mark's keys

- **Found:** 2026-10-24, the Arlington checks (F22)
- **Where:** the connector's emission (`emit/annots.rs`)
- **Severity:** wrong output (minor)
- **What happens:** a connector is written as a Link annotation with the
  keys every mark has. `/CA` is a Link key only from PDF 2.0
  (ISO 32000-2 Table 166); `/T` and `/CreationDate` aren't Link keys.
- **Expected:** a Link holds only Link keys, or the connector is another
  kind of annotation.
- **Repro:** `every_mark_kind_holds` without the allowance rows
- **Notes:** sheaf reads a connector's opacity back from `/CA`, and
  `/SheafAnnot`'s staleness digest covers the standard keys, so changing
  it is a design question. Held by `AnnotLink CA too-new` and
  `AnnotLink T` / `CreationDate` `unlisted`.

### B42: sheaf's marks never raise the file's PDF version

- **Found:** 2026-10-24, the Arlington checks (F22)
- **Where:** `Emission::needs` (marks have none), the planner
- **Severity:** wrong output
- **What happens:** marks write keys newer than the file's version:
  1.4 (`/NM`, `/CA`, the appearance's ExtGState `ca`/`CA`/`BM`), 1.5
  (`/CreationDate`, Square and Circle `/RD` and `/BE`, Caret, Polygon,
  PolyLine, `/IRT`, `/State`), 1.6 (`/RT`, `/IT`, `/CL`) and 1.7
  (FreeText `/RD`, `/BS`, `/LE`, pdf-issues #592; FileSpecification
  `/UF`; `/Measure`).
- **Expected:** the save raises the version, except where that would
  give up signatures (below).
- **Repro:** `a_pdf_1_3_file_holds`; 1,244 findings in the corpus tier
- **Notes:** a raise writes the catalog (M10.5 §11.7), which no
  signature allows (§9.3). F22's step 5 tried it (a per-kind
  `mark_needs` in `emit/annots.rs`, `cx.needs` in `Page::mark`,
  `needs_after` over the marks) and it broke sheaf-cli's
  `a_signed_file_appended_keeps_its_signatures` (corpus 0000581, a signed
  1.4 file). **Ruling (Greg, 2026-10-24):** keep §9.3's promise. Marks
  raise the version only when the save has no signatures to keep, and
  signed files keep the allowance; revisit with the rest of M11. The
  conditional raise is its own PR (gated paths), with a signed-file case
  so the allowance row stays used. Held by `AnnotText CreationDate
  too-new`.

### B43: The standard 14 fonts are written as `/BaseFont` alone

- **Found:** 2026-10-24, the Arlington checks (F22)
- **Where:** the font resources of headers, watermarks and mark
  appearances
- **Severity:** wrong output, in PDF 2.0 files
- **What happens:** a standard 14 font has no `/FirstChar`, `/LastChar`,
  `/Widths` or `/FontDescriptor`.
- **Expected:** PDF 2.0 asks every simple font for all four
  (ISO 32000-2 Table 109); before 2.0 they're optional for the
  standard 14 only.
- **Repro:** `a_pdf_2_0_file_holds`; corpus 0048641, 0113565, 0203319
- **Notes:** writing them (always, or in 2.0 files) changes every font
  resource sheaf writes, so it's its own PR with `just gates`. sheaf has
  the AFM metrics. Held by four `FontType1 … missing` rows that apply
  from 2.0.

### B44: An Append's new objects can take numbers the file refers to but doesn't have

- **Found:** 2026-10-24, the Arlington checks (F22)
- **Where:** an Append's numbering from `/Size`
- **Severity:** wrong output
- **What happens:** corpus 0117296's signature widgets have
  `/P 174 0 R`, an object the file doesn't have (`/Size 174`). The note an
  Append adds becomes object 174, so the widgets' `/P` names the note.
- **Expected:** new objects take numbers nothing in the file refers to.
- **Repro:** `just tier-arlington` on `forms/safedocs/0117296.pdf`
  without the allowance rows
- **Notes:** fixing it means knowing every reference in the file (the
  7k gate path). Also check whether a stable-numbering Rewrite can do the
  same. Held by 19 rows scoped to 0117296.

### B45: Arlington requires `/DA` on button fields

- **Found:** 2026-10-24, the Arlington checks (F22)
- **Where:** the model's FieldBtn* tables
- **Severity:** annoyance (a question for the model)
- **What happens:** a radio field that a save writes apart from its
  widget (BaseObjectsMissing), where neither the field nor the form has
  `/DA`, is a `missing` finding (0976876).
- **Expected:** ISO 32000-2 Table 228 asks `/DA` only of variable-text
  fields.
- **Repro:** `just tier-arlington` on `forms/safedocs/0976876.pdf`
  without the allowance row
- **Notes:** could go upstream with B40. Held by a row scoped to
  0976876.

### B46: The Arlington carried rule matches a merged object's finding against every object of the sources

- **Found:** 2026-10-24, the Arlington checks' review (F22)
- **Where:** `Report::beyond` (`crates/sheaf-doc/src/arlington/check.rs`)
- **Severity:** a gap in a test-only check
- **What happens:** a finding in an object sheaf merged over a source's
  dictionary is carried when any object of a source has a like one (the
  same table, key, rule and value, numbers aside; a `missing` finding's
  value is always "absent", and `too-new` compares none), not only the
  object it was emitted over. A page that loses its `/MediaBox` or
  `/Resources` in a save is missed when some page of the source lacks one
  too.
- **Expected:** carried only from the source object the merged one was
  emitted over.
- **Repro:** `a_merged_objects_finding_is_carried_from_any_object_of_a_source`
  pins the gap and flips with the fix.
- **Notes:** Built objects are never carried, so sheaf's own writers
  aren't affected; the corpus tier carried 18,670 findings (4,539
  `missing`). Fix: record each merged object's source object
  (`Hold::written` has each key: a page's, field's or widget's source,
  the primary's root objects, the same number in an Append), carry only
  from its findings, and triage what the tier then finds. `PageRes`,
  `Filtered` and `PagesNode` keys need their source worked out from their
  page.

## Features

### F1: Basic calculator

- **Found:** 2026-10-21, Greg, from a Gemini summary of a PDFlyer video,
  during M10.5
- **What:** PDFlyer's second calculator. Entry is the usual `a + b =`
  rather than an adding machine's amount-then-operator, and `=` totals
  the section. Otherwise it's the same tape: sections, a title row,
  notes, memory, and To PDF. PDFlyer's Settings choose which calculators
  appear.
- **Why:** Not everyone touch-types 10-key. The report lists the two
  modes as a core feature, but matching PDFlyer isn't a reason on its
  own (Greg, 2026-10-21); this needs users who want `a + b =` entry.
- **Overlap:** M9 slice 13 left it out on purpose: "PDFlyer's separate
  Basic calculator (the `=` key) is left out unless asked for"
  (`todos/m9.md`). If [F4](#f4-the-calculator-as-a-popup-not-a-panel)
  goes ahead, decide this with it.
- **Notes:** probably an entry mode on the same panel and tape model, not
  a second panel. Source:
  [UsingBasicCalculator](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/PDFlyer/UsingBasicCalculator.htm);
  `docs/prior-art/pdflyer.md` §2.3.

### F2: Percent key on the 10-key

- **Found:** 2026-10-21, Claude, while checking the same report
- **What:** a `%` key, as PDFlyer has ("calculates the percentage of
  the previously entered number"). sheaf's tape has add, subtract,
  multiply and divide only (`sheaf_edit::tape::Op`).
- **Why:** rates and allocations are common on workpapers.
- **Overlap:** M9 slice 13 took the other keys from PDFlyer; `%` isn't
  listed there, as taken or as left out.
- **Notes:** the behavior needs pinning down. CCH's own example
  (500 × 10% = 25000) is wrong, and adding machines differ. A new op
  is a new `Op` variant (in the ops JSON schema too) and a text form for
  `/Contents`, so other viewers show it. Tapes saved by earlier builds
  needn't read: sheaf is pre-release.

### F3: Put a calculator result on the page as text

- **Found:** 2026-10-21, Greg, from a Gemini summary of a PDFlyer video,
  during M10.5
- **What:** put a total or subtotal on the page as an editable text mark,
  or copy it to the clipboard, instead of placing the whole tape.
- **Why:** The report says PDFlyer does this ("results and subtotals can
  be pasted directly onto the workpaper as editable text"). CCH's help
  doesn't mention it; its calculators' only output is To PDF. Treat it
  as a sheaf extension, not parity.
- **Overlap:** M9 slice 14 (pick numbers from the page) is the reverse
  direction. The `FreeText` mark kind exists.
- **Notes:** a tape row's context menu ("Copy amount", "Place as text")
  would cover both.

### F4: The calculator as a popup, not a panel

- **Found:** 2026-10-21, Greg, during M10.5
- **What:** opening the calculator brings up a popup that takes focus.
  The user works the tape, then To PDF places it on the page as an
  annotation and the popup closes. Edit Tape on a placed tape opens the
  popup again with the tape loaded, and To PDF updates it where it is.
  This is PDFlyer's flow.
- **Why:** a calculation is an action the user takes. It should have
  focus while they do it and go away when they're done, with the result
  saved where they put it. Today's docked panel (`Panel::Calc`, opened
  by `workspace::ShowCalc`) stays on screen after the tape is placed,
  and looks and feels odd.
- **Overlap:**
  - M9 slices 13–14 built the panel: the tape format, Edit Tape, Place
    on the next page clicked, and Pick. The tape model
    (`sheaf_edit::tape`, the tape mark kind) stays; this changes the UI
    around it.
  - Plugins can open it: `show-panel` takes `"calc"` (`wit/sheaf.wit`;
    `BuiltinPanel::Calc` in `sheaf-plugin`). The call is fire-and-forget
    ("bring it into view"), which fits a popup as well as a panel, so
    the WIT needn't change. The one exception is its promise that it
    "doesn't take keyboard focus": either a plugin-opened calculator
    opens unfocused, or that line gets an exception for `"calc"`.
- **Open questions:**
  - **Pick and focus.** Pick (click a number on the page, or Shift+drag
    over a region) needs the page clickable while the calculator is
    open. So the popup can't be modal: it keeps keyboard focus while
    page clicks still reach the canvas.
  - **Where the tape lands.** PDFlyer puts it at the upper-left of the
    active page, and you drag it from there. sheaf's Place waits for a
    click on a page, which puts it in the right spot in one step. One
    option: To PDF closes the popup and the next click places the tape.
  - **Dismissing.** What Esc does to a tape that hasn't been placed:
    discard it, or keep it for next time (PDFlyer keeps the tape until
    New Tape). Today Esc clears the entry, then goes back to the
    document.
  - **Window or overlay.** A separate window can move to another
    monitor. A floating card inside the main window is simpler.
    `sheaf_ui::dialog` has only modal parts.
  - **Keys.** Choose them on their own merits. The differences from
    PDFlyer are in [B1](#b1-calculator-keys-dont-match-pdflyers), for
    reference.

### F5: Plugins ask for a command, not a panel; sheaf picks the view

- **Found:** 2026-10-21, Greg, discussing F4, during M10.5
- **What:** one host call that means "run this command; the user may
  want to set its parameters". It takes a command id, whatever arguments
  the caller has, and whether the user should see them before the
  command runs. sheaf decides what the user sees: a form generated from
  the command's schema, the command's own dialog, or a tool UI like the
  calculator popup ([F4](#f4-the-calculator-as-a-popup-not-a-panel)).
  It returns done or cancelled.
- **Why:**
  - It separates *what* from *how it looks*. `show-panel("calc")` names
    a view, but whether the calculator is a panel or a popup is sheaf's
    business, not the plugin's.
  - One path for every caller. Agents (M16), batch actions (M15), key
    bindings (`args-json`), the CLI and plugins all need commands with
    arguments, and one schema serves them all.
  - With a schema, sheaf can generate a form for any command, built-in
    or plugin, that has no dialog of its own.
- **Overlap:**
  - M16 and `todos/agent.md` ("Running commands live") plan most of
    this, for agents: a command with an argument schema runs without its
    dialog, and one without opens its dialog pre-filled and returns done
    or cancelled. The schemas are shared with M15. F5 extends that to
    plugins, and lets the caller ask for the user to review even when
    the arguments are complete.
  - The WIT today has `show-panel` (brings a view into view, without
    focus), `show-form` (shows the plugin's own form and waits for the
    answers), and the export `run-command` (sheaf calls a plugin
    command with `args-json`, which has no schema). A plugin has no
    import for running a command of sheaf's or another plugin's.
  - Not in M10.5: plugin API 0.4 (slice 20) doesn't touch these calls,
    and adding the import later only adds to the WIT.
- **Open questions:**
  - **Views stay.** Not every `show-panel` call is a tool:
    `currency-convert` calls `show-panel("comments")` to show its
    results. Panels that are views (pages, bookmarks, comments, a
    plugin's own panels) keep `show-panel`. Tools that end in an edit
    (the calculator, Set Status, Bates, OCR) go through the new call.
    The calculator is a tool that was filed as a panel.
  - **Example:** the `subtotal` plugin shows a region's sum in the
    status bar. With this call it could open the calculator pre-filled
    with those amounts, for the user to check and place.
  - **Permissions: permissive for now** (Greg, 2026-10-21: "while we are
    in pre-release I'm inclined to be permissive. we want to have low
    friction"). Any plugin may run any command, with or without the
    user seeing its arguments, and there's no per-command allowlist.
    Revisit before the first release:
    - which commands may run unprompted (M16 asks the same for agents:
      "commands an agent should never run");
    - whether running a command bypasses the manifest permissions some
      ops need (crop, watermark, flatten; M16 "Decide at kickoff").
  - **Blocking.** `show-form` holds the plugin's call until the user
    answers, outside its time limit, but a calculator session can be
    long. Keep that model, or return at once and report the result
    through an event?
  - **Return value:** done or cancelled, the version the command made,
    and perhaps the arguments the user ended with.
  - **Precedent:** Emacs's `interactive`, where a command says how to
    ask for its arguments, `call-interactively` asks, and a direct call
    doesn't. Also VS Code's `executeCommand(id, ...args)`.

### F6: Make Go to Page easy to find

- **Found:** 2026-10-21, Greg, during M10.5, who asked for "goto page#"
  as useful in large documents
- **What:** Go to Page exists (M7, `crates/sheaf-app/src/goto.rs`). It
  takes a page number or a label ("iv", "A-3"). But the only ways in
  are View > Page Navigation > Go to Page, Ctrl+Shift+N (Acrobat's key)
  and the command list. Make it easy to find:
  - **Click the page indicator.** The status bar's "page 4 / 120" is
    plain text inside one string (`DocView`'s status line). Clicking it,
    or making it an editable box, should open Go to Page, as page boxes
    do in Acrobat, Foxit, pdf.js and Evince.
  - **Bind Ctrl+G as well.** `docs/prior-art/README.md` recommended
    binding both, because Foxit users expect Ctrl+G and F3 already finds
    the next match. Its note that "sheaf has no go-to-page command yet"
    is out of date.
- **Why:** jumping to a page is a basic move in a large binder, and a
  command found only through a submenu and an Acrobat key is easy to
  miss. Greg didn't know it existed.
- **Overlap:** M7 built the command; `todos/toolbar.md` lists it among
  the View items. Nothing plans a page box.
- **Notes:** small. Check how long a label like "A-3 (57 of 1,240)"
  can get before it crowds the status bar.

### F7: Scrollbars on everything that scrolls

- **Found:** 2026-10-21, Greg, during M10.5
- **What:** a visible scrollbar on every area that scrolls, starting
  with the page view's panes. Each pane gets its own bars, vertical and,
  when zoomed wider than the view, horizontal. Drag the thumb to move;
  click the track to page.
- **Why:**
  - In a large document the thumb shows roughly where you are at a
    glance, without reading "page X of N".
  - With the view split, each pane can be at a different point in the
    document, and only one pane's position is in the status bar.
  - Today the wheel and keys are the only way to scroll. A thumb gives a
    coarse jump that the wheel can't.
- **Overlap:** nothing plans scrollbars. Neither sheaf nor GPUI draws
  one: GPUI only reserves space (`scrollbar_width`) and reports the
  platform's auto-hide setting (`App::should_auto_hide_scrollbars`). Go
  to Page ([F6](#f6-make-go-to-page-easy-to-find)) is the precise
  jump; this is the at-a-glance one.
- **Notes:**
  - **One component** in `sheaf-ui`, used by all of them: about 19 files
    have an `overflow_*_scroll` area (the page panes, the side panels,
    the palette, the dialogs' lists, the calculator tape). Virtualized
    lists know their full height already (`sheaf_ui::list::ListLayout`),
    and so do the page panes (their layout's `total_h` and `view_h`).
  - **Source:** the scrollbar itself is `gpui-base`'s `scrollbar.rs`
    (0.7.3, Apache-2.0): about 1,850 lines plus 1,040 of tests.
    `gpui-component`'s `scroll/scrollable.rs` only wraps it in a
    `.vertical_scrollbar(&handle)` extension. Worth reading:
    - its `ScrollbarHandle` trait, implemented for `ScrollHandle`,
      `UniformListScrollHandle` and `ListState`;
    - its three modes: show while scrolling, on hover, or always;
    - its thumb geometry.

    It's tied to kit's theme and animation modules, so using it as the
    reference for a smaller one of our own probably beats copying it.
    `todos/agent.md` cleared the kit crates for copying with
    attribution, though not as a dependency. Zed draws its own in its
    `ui` crate; check that crate's license before reading it for code,
    since the Zed crates `agent.md` checked are GPL-3.0.
  - **One page at a time.** In the single-page and single-spread modes,
    a pane only scrolls within the current spread, so a plain scrollbar
    would show position within the page. Acrobat's single-page bar spans
    the whole document and steps by page; consider the same.
  - **While dragging,** show the page it would land on ("iv (4 of
    120)"), as Acrobat and Preview do.
  - **Large files open lazily.** The page count can grow while a file
    is read ("120+ (searching…)"), so the thumb shrinks as it reads. It
    shouldn't jump under the user's drag.
  - **Overlay or always shown:** follow the platform's auto-hide setting.
  - **Later, maybe:** marks on the track for search hits, bookmarks or
    comments, like an editor's overview ruler.

### F9: Test against pdf-corpus's signatures/ and conformance/ collections

- **Found:** 2026-10-22, Greg, during M10.5 slice 18
- **What:** corpus tiers over `../pdf-corpus/signatures/` (531 files,
  checked with pyHanko: `intact`, `valid`, changed after signing,
  digest mismatches, several subfilters) and `conformance/` (3,524:
  PDF/A, PDF/UA and PDF/X claims, some damaged). Slice 18 added
  `Corpus::scans()` and tiers over `scans/`; these two aren't read yet.
- **Why:**
  - `signatures/`: Append must keep signatures that hold, and the
    signature asks must be right (B5). Today's signed-file tests use
    the 322 `/DocMDP` forms.
  - `conformance/`: the PDF/A save Note (M10.5 slice 16), PDF/A
    convert and validate (M14), PDF/UA (M17).
- **Overlap:** M10.5 slice 19's exports and signature asks; M12
  (digital signatures); M14; M17.
- **Notes:** each needs a `Corpus::` reader for its manifest, like
  `scans()`, and a tier with its own `SHEAF_…_ONLY` narrowing.
  - **Progress (M10.5 slice 19):** `Corpus::signatures()` reads
    `signatures/`'s manifest; sheaf-cli's `tests/edit.rs` uses it (an
    `--append` onto signed files keeps their signatures).
  - **Progress (B5):** the first tier over `signatures/`, sheaf-doc's
    `the_corpus_s_signatures_agree_with_pyhanko` (every signed file's
    digests against pyHanko's `intact`; a few seconds, so not ignored;
    `SHEAF_SIGS_ONLY` narrows it). The Append and signature-ask tiers
    over the collection, and `conformance/`, are still to come.
  - **veraPDF** (2026-10-23, the build-or-adopt audit): the
    `conformance/` tier compares sheaf's checks with veraPDF 1.30's JSON
    (`--format json`, flavours `1a`…`4f`, `ua1`, `ua2`). veraPDF needs
    Java, which this machine doesn't have, or the `verapdf/cli` Docker
    image, so the tier skips without it, as corpus tiers do without the
    corpus ([build-or-adopt.md](build-or-adopt.md), D3).

### F13: In-house what sheaf uses of lopdf and drop the dependency

- **Found:** 2026-10-22, Greg and Claude, during M10.5 slice 19
- **What:** a long-term direction, not scheduled, and never mid-milestone
  (Greg, 2026-10-22). Bring what sheaf uses of lopdf into sheaf's own
  crates and drop the dependency, while still sending upstream any fix
  found along the way. It isn't either/or.
  - **Already sheaf's own:** the writer (`serialize/`), the content-stream
    code (`content/`) and the lazy-reading layer (`source/`).
  - **Still lopdf's:**
    - the object model;
    - the object parser and cross-reference reading;
    - object streams;
    - stream decompression;
    - encryption, which sheaf would keep on the RustCrypto crates and
      not hand-roll;
    - the eager `Document`: the Rewrite graph, PDFs built in memory,
      `prune_objects`;
    - its writer, used only by `annot::tape_pdf`, tests and fixtures.
  - About 9k of lopdf's 23.5k lines are off sheaf's production paths:
    text encodings and glyph tables, CMap parsing, outlines, bookmarks
    and TOC, fonts, dates, XObject helpers, IncrementalDocument.
- **Why:** sheaf carries a vendored, patched lopdf whose patches reshape
  its reader, encryption and object model. The vendored crate is outside
  the workspace, so sheaf's clippy `-D warnings` and lint bans don't run
  on it, and each new upstream commit means reapplying the patch.
- **A first step:** move `LazyPdf` (`vendor/lopdf/src/lazy.rs`, 989
  lines) into sheaf-doc's `source/`.
  - The byte sources and caps (`ByteSource`, `FileBytes`, `ReadLimits`,
    `parse_from`) stay in lopdf, whose shared `XrefReader` reads through
    them.
  - lopdf then makes public what LazyPdf reads today as crate-private:
    from reader.rs, `XrefReader`, `Resolved`, `decryption`, `encrypts`,
    `header_offset`, `length_value`, `normal_offsets`, `object_bound`;
    from the parser, `Resolve`, `header`, `indirect_object_in`,
    `stream_end`.
  - Keep `source::lazy_tests` identical across the move: every object of
    the corpus's readable forms, read lazily, equals the eager load.
- **Upstream meanwhile** (Greg: the weekend of 2026-10-24/25, each pull
  request confirmed first): `vendor/lopdf/SHEAF.md` lists fixes that
  stand on their own:
  - each xref stream read once;
  - lenient `/XRefStm` past EOF;
  - white space around `startxref`;
  - `/Encrypt null`;
  - strings in stream dictionaries (lopdf #603);
  - `/EFF`;
  - V4+ filter names and `/Identity`;
  - the set in `traverse_objects` and `prune_objects`.
- **Upstream since** (2026-10-23, the build-or-adopt audit, read in
  `J-F-Liu/lopdf` main at `d0b3abf`, not run; [build-or-adopt.md](build-or-adopt.md), entry 2):
  - 0.46.0 (2026-10-21) has #598 and the stream-dictionary strings
    (#603, `2f497f8`).
  - Merged since: V4+ `/Identity` defaults (#624) and
    `traverse_objects` and `prune_objects` in linear time (#630), both
    still to confirm as the same fixes as sheaf's; caps in object
    streams and predictors (#625); rebuilding files that have only
    cross-reference streams (#613, which the audit placed both in 0.46.0
    and after it).
  - Still to send: `/EFF`, `/Encrypt null`, white space around
    `startxref`, and each xref stream read once and lenient `/XRefStm`,
    if still needed.
  - Diff each `sheaf:` hunk against upstream main before writing its PR.
- **Overlap:** `vendor/lopdf/SHEAF.md`, "Going back to crates.io";
  F15 (a Rewrite that doesn't load the file whole) would remove the
  largest use of the eager `Document`.
- **Notes:** to size at triage: the object model's reach (about 400
  import sites), and the parser's edge cases, which lopdf's parser
  fixes and the corpus comparisons encode (`source::lazy_tests`; the
  corpus runs).
- **Maybe published** (Greg asked, 2026-10-22): the in-housed lopdf parts,
  `LazyPdf`, sheaf-doc's writer (`serialize/`), its `source/` and
  `geom`, and the generic half of `content/` (lex, writer, splice) could
  become one layer crate, and later perhaps a published alternative to
  lopdf, as `in-progress` and `instance-handoff` were extracted.
  - **Production coupling today:**
    - `serialize` reaches `materialize` (`ObjKey`) and `source`;
    - `source` reaches `content`, `geom` and `store` (`BlobId`);
    - `content` reaches `annot` and `private`, which are sheaf's blocks
      and markers.
  - Make it an internal crate with a clean boundary first; decide on
    publishing after an ecosystem survey.
  - **The survey** (2026-10-23, the build-or-adopt audit): no published
    crate offers an owned, f64, order-keeping, lazy, capped object layer
    with byte spans, so a crate of that layer would have a real niche.
    The nearest are hayro-syntax (read-only, borrowed objects) and lopdf
    (eager, f32).
- **Deferred** (Greg, 2026-10-22): revisit after more milestones, since
  what sheaf needs from this layer may change as later scopes dig in.
  Upstream fixes go ahead meanwhile.

### F15: A Rewrite that doesn't load the whole file (the flatten export's memory gate)

- **Found:** 2026-10-22, M10.5 slice 19 (the flatten gate)
- **What:** a Rewrite that copies the primary's objects without loading
  the file whole: a streaming copy, its pruning and `keep_only`
  included.
- **Why:** the plan's gate for Export Flattened on the heavy 7k test
  file (peak ≤ 1.5× the file + 200 MB, 220.5 MB, in the flatten's own
  process) can't be met by construction. Opening the heavy alone peaks
  at 221 MB, and the export peaks at 513–515 MB, most of it the
  Rewrite's eager load of the primary (`materialize::graph::eager`; a
  plain Rewrite adds 252 MB). The gate is measured on every run and
  asserted only with `SHEAF_GATE_FLATTEN`, like proposal 8's accepted
  misses. Every Rewrite-based save and export of a large file pays this.
- **Overlap:** M10.5 proposal 37 (a symbolic `keep_only`, unbudgeted);
  F13 (the eager `Document` is lopdf's); M14 (object-stream packing).
- **Notes:** the gate may also want restating as what the export adds
  over the open (293 MB today against 220.5) rather than the process's
  peak; decide with the design. Slice 19's notes have the five runs.

### F10: Redo OCR: replace another tool's text layer

- **Found:** 2026-10-22, Greg, comparing sheaf's OCR with Acrobat's and
  ocrmypdf's, during M10.5
- **What:** a third choice in Recognize Text (OCR)…, beside skipping
  pages with text and recognizing every page: redo. On each page, take
  out the invisible text another tool's OCR left (text render mode 3:
  Acrobat's Searchable Image, ocrmypdf, ABBYY, a scanner's own), keep
  the visible text, and write sheaf's layer in its place.
- **Why:**
  - With skip off, sheaf's layer goes on top of the old one, so the page
    draws its words twice, invisibly: Find hits each word twice, and
    selection, copy and plugins' page text are doubled. A plugin's
    `recognize` always runs with skip off
    (`crates/sheaf-app/src/plugins.rs:898`).
  - With skip on (the default), a page whose layer came from a weak
    engine or a scanner can't be improved. Acrobat's only way is to
    rasterize the page, which loses its real text.
  - A page under `HAS_TEXT_MIN_CHARS` (20, `crates/sheaf-ocr/src/job.rs`)
    is recognized even with skip on, so a short foreign layer or a saved
    Bates number comes out twice.
- **Overlap:**
  - [F8](#f8-a-command-to-remove-recognized-text) removes sheaf's own
    layer on its own; redo is that plus recognizing again, for any
    tool's layer. [B7](#b7-clear-ocr-text-silently-does-nothing-on-a-damaged-old-layer)
    is the case where even sheaf's own old layer can't be taken out.
  - Sheaf already replaces its own layer: `text_layer::install` strips
    streams marked `/SheafOcrLayer` before writing
    (`crates/sheaf-doc/src/text_layer.rs`), and slice 18's OCR blocks
    (`/SheafOcrBlock`, `todos/m10.5-edit-api.md` decision 26) keep that a
    dict edit. This entry is the foreign case, which has no marker, so
    the page's own streams must be decoded and rewritten. Sheaf never
    does that today.
  - M11's true content removal and Remove Hidden Information
    (`todos/m11.md`) take text out of a page's streams. Redo needs the
    same operation for invisible text alone, so it probably follows M11
    or shares its machinery.
- **Notes:**
  - **Prior art:** ocrmypdf's `--redo-ocr` (now `--mode redo`) strips
    invisible text and recognizes again, keeping visible text;
    `--force-ocr` rasterizes instead, and `--skip-text` is sheaf's
    default. Acrobat refuses a page with "renderable text" (hidden text
    counts) and has no redo. ABBYY has a setting to force OCR. Foxit's
    behavior is unconfirmed.
  - **Which invisible text is OCR:** not all of it is. Authors use
    render mode 3 for search-only or copy-only text too. Strip every
    mode-3 run, or only those over a page-sized image?
  - **Pages with visible text:** the page's render shows that text, so
    OCR reads it again. Either recognize only outside PDFium's
    character boxes, or drop recognized words that overlap them.
    Check how ocrmypdf's redo handles this.

### F11: Plugin memory and time limits in settings.toml

- **Found:** 2026-10-22, Greg, discussing a wasm build of qpdf as a
  repair plugin, during M10.5
- **What:** the plugin limits, today fixed at 256 MiB of linear memory
  and 30 s a call (`Limits::default()`,
  `crates/sheaf-plugin/src/runtime.rs:37`), set in `settings.toml` for
  every plugin, and possibly overridden plugin by plugin.
- **Why:**
  - A plugin that works on a whole file, such as a qpdf repair or
    another document transform, needs more than 256 MiB for a large
    binder, and more than 30 s.
  - A user on a small machine may want them lower.
  - Today, changing them means rebuilding sheaf.
- **Overlap:**
  - `todos/plugins/document-transforms.md`, Open: may a transform
    declare raised limits (up to about 3.5 GiB and 15 minutes), shown
    at consent? That is the plugin asking. This entry is the user
    setting them, and the two need one rule for which wins.
  - `docs/plugins.md`, Limits, documents today's fixed values.
- **Notes:**
  - **Where per-plugin values go:** not in `[plugins.<id>]`. That
    section is the plugin's own: it's handed to the plugin as JSON
    (`Prefs::plugin_json`), and keys its manifest doesn't name are
    reported as typos. A separate table such as `[plugin-limits]` with
    `[plugin-limits.<id>]` keeps the host's settings out of the
    plugin's reach.
  - **Plumbing:** the runtime is made once with one `Limits`
    (`crates/sheaf-app/src/plugins.rs:358`), but each plugin has its own
    store (`Instance::start`, `runtime.rs`), so a per-plugin value can
    come in through `LoadOptions`.
  - **Changes while running:** settings are re-read when the file
    changes. A timeout can apply from the next call; a memory limit is
    set when the store is made, so it applies when the plugin is
    reloaded.
  - **Bounds:** wasm32 memory can't pass 4 GiB. A value out of range is
    clamped and reported, as other settings problems are, never fatal.
  - The timeout already leaves out time spent waiting on the user and
    on OCR. Keep that.

### F12: A qpdf plugin

- **Found:** 2026-10-22, Greg, discussing PDF repair, during M10.5
- **What:** qpdf built for wasm as a plugin of its own, kept in its own
  repository and installed as an archive (Plugins > Install, through
  `install_archive`), not bundled with sheaf. Commands such as:
  - check a file (`--check`), as a second opinion beside sheaf's reader;
  - linearize, and pack object streams, before M14 does them natively;
  - rewrite a damaged file, as Save As does today, by qpdf's rules;
  - dump the file's objects (`--qdf`, `--json`) for debugging.
- **Why:** it brings qpdf's checks and writer options to sheaf users
  now. It would also be the first plugin built outside this
  repository, and the first real user of the document-transform API:
  if a qpdf plugin works end to end, that API is probably right.
- **Overlap:**
  - `todos/plugins/document-transforms.md`, whose "What can't work"
    says a tool like qpdf needs a wasm build. It also records the `gc`
    requirement below.
  - M14 Optimize (`ROADMAP.md`): linearization and object streams,
    which make the plugin's versions a bridge, not the long-term path.
    M14's Fast Web View entry rules out a transform plugin as the way
    sheaf linearizes, since a plugin parses and writes the whole file
    again; that cost is acceptable for a stopgap.
  - [F11](#f11-plugin-memory-and-time-limits-in-settingstoml).
- **Notes:**
  - **Needs, in order:**
    1. wasmtime's `gc` feature in sheaf's build. qpdf uses C++
       exceptions throughout, and wasm exception handling needs `gc` in
       wasmtime 49. Without it the component is rejected at load.
    2. The document's bytes. Today's API has no way to hand a plugin
       the whole file; the document-transform API would add it. Until
       then, a file field in a plugin form returns a path, and the
       plugin can open it only inside the folders its manifest's
       read/write permissions grant (preopened in
       `crates/sheaf-plugin/src/runtime.rs`). So it can work on files
       on disk, not on the open document.
    3. Higher memory and time limits (F11): 256 MiB and 30 s won't hold
       a large file.
  - **Why not in this repository:** the build needs a C++ wasm
    toolchain (wasi-sdk, libc++ built with exceptions, and zlib and
    libjpeg built for wasm). Sheaf's CI and `package.sh` build only
    Rust plugins.
  - **License:** qpdf is Apache-2.0, so the plugin ships qpdf's NOTICE.

### F16: Turn QR codes into links

- **Found:** 2026-10-23, Greg, during M10.5
- **What:** a plugin command that finds the QR codes on a document's
  pages, decodes them, and puts a link over each one that holds a web
  address, so clicking the code opens what scanning it would. Before
  adding anything, it lists what it found (the page, the decoded text,
  and whether it will become a link) for the user to choose from.
  - `http`, `https` and `mailto` addresses become links. Other schemes
    (`javascript:`, `file:`, app links) are listed and skipped.
  - Other content (plain text, a contact card, Wi-Fi details, a payment
    code) doesn't become a link. The plugin can offer to put it in a
    note mark beside the code instead.
- **Why:** QR codes on invoices, statements, letters and tax notices
  lead to payment portals and sign-in pages. On screen, the only way to
  follow one is to point a phone at the monitor. A link helps
  screen-reader users too, who can't scan a code at all. And seeing the
  decoded address before it's linked shows a preparer where a code
  really goes before trusting it.
- **Overlap:**
  - M13 link editing (`ROADMAP.md`), whose "create links from URLs in
    text" is this feature's text-based sibling.
  - Pending's "Follow links in the page view". Sheaf doesn't follow
    links yet, so until it does, these work only in other viewers.
  - M5's separator sheets read barcodes. `todos/m5-intake.md` ("Reading
    and writing codes", on the `m5-intake` branch) benchmarked rxing and
    zxing-cpp; both read QR codes with 0.34 mm modules in every
    1-bit case tested at 200 and 150 dpi.
- **Notes:**
  - **How:** render each page in gray through the plugin API's `render`
    interface, decode QR codes only with rxing (pure Rust, Apache-2.0,
    builds for wasm), and turn each code's corners into the link's
    rectangle with `image.to-page`. Read QR only: in M5's bench,
    reading every format over a whole page found false codes on about
    22 of 150 real scans. Check the false-read rate with QR alone.
  - **Needs, in order:**
    1. An op that creates a link. The edit vocabulary has none: plugins
       add marks and connectors, and a link is only something they may
       not delete. A plugin-made link that opens an address may need the
       user's approval (`approval`), or the plugin's own confirmation
       list may be enough. Decide when the op is designed, probably
       with M13's link editing.
    2. Seeing the links already there. `annotations()` never lists a
       link, so a second run can't tell which codes are already linked
       and would stack duplicates. Either the document interface lists
       links (rectangle and target), or the op skips a link with the
       same target over the same area.
  - **Where the decoder lives:** in the plugin, or, once M5 puts a
    reader in sheaf, in a host `barcodes` interface beside `ocr`, which
    saves every plugin from bundling rxing.
  - **Home:** an in-repo plugin beside `subtotal` and `amount-crossref`.
    rxing is pure Rust, so the build needs nothing new.
  - **Size:** the plugin is small; the link op is the real work.
  - **When:** revisit after M13 lands (Greg, 2026-10-23).

(F17 is on the content interpreter PR.)

### F18: Notices for the data and code sheaf ships

- **Found:** 2026-10-23, the build-or-adopt audit
- **What:** `packaging/licenses` carries each notice as its item
  arrives:
  - Unicode-3.0, for unicode-normalization from 0.1.26 (which declares
    it) and ICU4X data;
  - Adobe's BSD-3 notice for hayro-cmap's CMap bundle (the content
    interpreter PR brings it);
  - the OFL for Noto Sans and Liberation (M10 slices 7 and 11);
  - BSD-3 for Foxit's Symbol and Dingbats, if M14 takes them;
  - the IJG notice ("based in part on the work of the Independent JPEG
    Group") for jpeg-encoder, if M11 takes it;
  - SCOWL's notice, if spell check ships en_US.
- **Why:** each license requires its notice in binary distributions;
  today `packaging/licenses` has none of these.
- **Overlap:** M10 slices 7 and 11 (`m10.md:1469`, `:1597`); M11's
  content interpreter (CMaps) and slice 6 (JPEG); M14's embed-all
  profile; Spell check (Pending); B27, whose license check covers crates,
  not data files.
- **Notes:** the lockfile is at unicode-normalization 0.1.25, whose
  license field is still MIT OR Apache-2.0; the next `cargo update`
  brings 0.1.26. [build-or-adopt.md](build-or-adopt.md), entries 19, 21, 26, 35, 63 and 84.

### F19: Wipe file keys, not only passwords

- **Found:** 2026-10-23, the build-or-adopt audit
- **What:** keep an encrypted file's key in `Zeroizing` memory from
  derivation to drop. lopdf's `EncryptionState` holds it in a plain
  `Vec`, and `serialize::crypt::Crypt` keeps an HMAC keyed with it.
  Passwords are already `Zeroizing` (`store.rs:42`).
- **Why:** hygiene. A key in a crash dump or swap opens the file without
  the password.
- **Overlap:** F13 (in-house encryption on the RustCrypto crates); M12's
  PINs and keys (secrecy comes with cryptoki).
- **Notes:** best-effort, since GPUI's text input and PDFium's copies of
  the password can't be wiped; say so in the docs. [build-or-adopt.md](build-or-adopt.md), entries 29 and
  43.

### F20: Upstream: cms `CertificateChoices` alternatives for attribute certificates

- **Found:** 2026-10-23, the build-or-adopt audit
- **What:** cms 0.3.0-rc.0's `CertificateChoices` has only
  `Certificate` and `other [3]` (`src/cert.rs:30-35`), so a SignedData
  whose certificate set holds an old attribute certificate (`[1]`,
  `[2]`) fails to decode. One of the corpus's 593 signatures does, in
  the audit's B5 spike. Send a fix to RustCrypto/formats.
- **Why:** M12's verify panel, and B5, would otherwise report "can't
  read" for such a signature.
- **Overlap:** B5; M12 slice 3.
- **Notes:** small: an issue first, then a PR. [build-or-adopt.md](build-or-adopt.md), entry 44.

### F21: Try PP-OCRv6 in the PaddleOCR engine

- **Found:** 2026-10-23, the build-or-adopt audit
- **What:** offer PaddleOCR's PP-OCRv6 models (tiny, small or medium) in
  place of, or beside, the PP-OCRv5 mobile models `sheaf-ocr` loads
  today (`crates/sheaf-ocr/src/data.rs:101-111`).
- **Why:** PP-OCRv6 (2026-06-11) claims +5.1% recognition and +4.6%
  detection over PP-OCRv5_server (vendor figures). oar-ocr 0.10.0, which
  sheaf already uses, has a `pp_ocrv6` preset, so this is a model and
  dictionary swap, not a new dependency.
- **Overlap:** M5 OCR (done); `todos/ocr.md`'s model sets; F10 (Redo
  OCR).
- **Notes:** measure on `../pdf-corpus/scans/`: accuracy against
  PP-OCRv5 and Tesseract, time per page, model size. The v6 weights'
  license is unverified. `scripts/fetch-ocr-data.sh` would fetch the new
  files. [build-or-adopt.md](build-or-adopt.md), entry 13.

### F23: hayro-syntax as a second reader in the lazy-reader corpus test

- **Found:** 2026-10-23, the build-or-adopt audit
- **What:** a dev-dependency on hayro-syntax 0.8, and a corpus tier that
  compares sheaf's lazy reads (object by object, or at least each page's
  resources and contents) with hayro-syntax's, with the known
  differences listed, as `source::lazy_tests` does for the eager reader.
- **Why:** F13 in-houses the parser and the reading of cross-references.
  Today the only in-process comparison is lopdf's eager reader, which
  shares sheaf's code. An independent pure-Rust reader catches
  regressions a shared code path can't.
- **Overlap:** F13; `source::lazy_tests`; F17 (hayro-interpret as a
  second corpus oracle, from the content interpreter PR); the hayro
  "oracle only" rule (M10.5's Not in M10.5).
- **Notes:** Apache-2.0 OR MIT, `no_std` + alloc, no unsafe by default.
  It needs whole-file bytes (an mmap works). Its repair differs from
  PDFium's, so expect a triage list. Decide at F13's triage, not
  mid-milestone. [build-or-adopt.md](build-or-adopt.md), entry 1.

### F24: sheaf's private names as second- or third-class names

- **Found:** 2026-10-24, the Arlington checks (F22)
- **What:** rename the keys `/SheafAnnot` (a mark), `/SheafBlock` (a
  block's stream) and `/SheafGenerators` (the catalog), and the
  marked-content names `/SheafGen` and `/SheafOcrBlock`
  (`private::KEYS`), to a registered second-class prefix (e.g.
  `SHEF_Annot`) or third-class `XX` names, or move the page and form data
  under `/PieceInfo`, the spec's place for an application's private data.
  Or keep them.
- **Why:** ISO 32000-2 Annex E keeps first-class names for the spec. A
  validator could flag them, or a future spec key could collide.
  Arlington counts them as unlisted "other"; the checker exempts them by
  name from the `unlisted` rule, and a renamed set would need no
  exemption.
- **Overlap:** M10.5's private data (`private.rs`); the sidecar format
- **Notes:** pre-alpha, so a rename has no compatibility cost (clean over
  compat). A decision for Greg. Registering a prefix is with the PDF
  Association's registry.

## Closed

### B1: Calculator keys don't match PDFlyer's

- **Closed:** 2026-10-21. Not a bug: sheaf doesn't copy PDFlyer just
  for the sake of it (Greg). Keys are chosen on their merits, with
  [F4](#f4-the-calculator-as-a-popup-not-a-panel).
- **Found:** 2026-10-21, Claude, checking the Gemini summary against
  `docs/prior-art/pdflyer.md` §3
- **The differences**, from CCH's
  [Using10keyCalc](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/PDFlyer/Using10keyCalc.htm)
  and the typed-key map in `crates/sheaf-app/src/calc_panel.rs`:

  | Key | PDFlyer | sheaf |
  |---|---|---|
  | `*` | multiply (the keypad's ×) | total (an adding machine's total key) |
  | G | MC | M+ |
  | H | MS | M− |
  | J | MR | MR |
  | K | M− | MC |
  | L | M+ | MS |
  | `T` | date and time into the note | nothing (Ctrl+T while editing a note) |

- **Notes:** M9 slice 13 chose the memory order without PDFlyer's table
  to hand (`todos/m9.md`).

### B5: A file whose signatures were broken still asks about them on a reorder

- **Closed:** 2026-10-23, fixed in the B5 PR. A
  signature a save would break or flag is checked first
  (`forms::sigs::integrity`, `Signature::integrity`): its `/ByteRange`
  hashed from the source's bytes against the digest its CMS signed (the
  messageDigest signed attribute, a document timestamp's messageImprint,
  or for no signed attributes the DigestInfo its RSA PKCS#1 v1.5 value
  opens to with the signer's certificate's key). One that no longer
  matches is `Verdict::AlreadyBroken` and doesn't ask; one that can't be
  checked (`Integrity::Unknown`: no signed attributes and not RSA, GOST,
  a CMS that can't be read) still asks. The form check reports one that
  fits its revision as `BrokenSignature`; `sheaf inspect --json` counts
  them (`signatures_broken`). Tests: sheaf-doc
  `forms::sigs::integrity::tests` (`a_signature_is_intact_until_a_byte_it_signs_changes`,
  `a_timestamp_is_checked_by_its_imprint`,
  `an_rsa_signature_with_no_signed_attributes_is_opened`,
  `what_can_t_be_checked_is_unknown_and_asks`,
  `byte_ranges_are_hashed_as_given_and_cut_at_the_end`,
  `a_byte_range_that_isn_t_pairs_is_unknown`, `bytes_before_the_header`,
  `ber_is_read`,
  `hostile_cms_is_unknown_not_a_crash`, `checks_are_shared_capped_and_lazy`,
  `a_broken_signature_asks_nothing`, and the first tier over
  `signatures/`, `the_corpus_s_signatures_agree_with_pyhanko`),
  `forms::tests::a_signature_whose_bytes_changed_is_found`,
  `materialize::plan_tests::a_section_writing_the_catalog_over_a_broken_signature_appends`;
  sheaf-ops `forms_saves::a_signature_a_rewrite_broke_isn_t_asked_about_again`
  (the repro); sheaf-cli `edit::a_signature_broken_already_isn_t_asked_about`
  (0008830, 0010094, and a file `--signatures break` wrote). The header
  tier on 62 signed forms (the first in `forms/`'s order with a signed
  field): 72 of the 74 reorders asked about the signatures (and were
  appended anyway), now 36. The 36 that stopped asking are 35 reorders
  after a forced Rewrite and 0008830's reorder of the file as it came
  (its signatures were broken already); the 36 that still ask reorder
  a file as it came, with its signatures intact.
  The mark tier: 359 ask, from 360; 7334180's signature, whose
  `/ByteRange` doesn't span its own `/Contents`, no longer asks before
  its Rewrite (`BaseUnsound`). The crop tier: 1104 ask, from 1121; the 17 that
  stopped (each signature stale or broken when it came) append, but
  7334180, rewritten (`BaseUnsound`). The form check's comparison
  (`Report::added_since`) counts a signature it found stale or broken
  before the save as nothing new, whatever the file's size after
  (`forms::check::tests::a_signature_broken_already_is_nothing_new`).

- **Found:** 2026-10-22, M10.5 slice 17's corpus tier (pre-existing)
- **Where:** `forms::sigs::sig_impact` (`crates/sheaf-doc/src/forms/sigs.rs:187`)
- **Severity:** annoyance
- **What happens:** a file rewritten with `break_signatures` (the user
  already agreed to break them) still gets the signature ask when its
  pages are reordered. `sig_impact` judges by `/V` and the permissions,
  not by whether the signed bytes still verify. This is 25 of the
  62-file sample's 100 reorder Appends.
- **Expected:** a signature whose digest no longer matches its bytes
  doesn't ask again.
- **Repro:** append_corpus's header tier (slice 17) on a signed form
  after a forced Rewrite, then reorder.
- **Notes:** probably needs the digest check pyHanko does
  (`../pdf-corpus/signatures/` has files changed after signing and
  digest mismatches to test against; see F9).
- **Also (M10.5 slice 19):** the exports' and the command line's
  signature ask reuse `sig_impact`, so (both fixed with it):
  - a signature already invalid when the file was opened still asks:
    `signatures/safedocs/0008830.pdf` and `0010094.pdf`, which pyHanko
    finds neither intact nor valid, exit 3 on a mark add (37
    unencrypted files in `signatures/` have such a signature). Both
    are signed with no signed attributes, so their digests are opened
    from their RSA values. Of `signatures/`, pyHanko gives a verdict of
    not intact to 17 signatures in 15 files, and sheaf finds exactly
    those broken; the manifest's other not-intact signatures are
    pyHanko's errors (59 document timestamps its run refuses, GOST
    digests, asn1crypto parse errors);
  - a file written with `sheaf … --signatures break` asks again on its
    next edit (`append_anyway: true`, "… removed since the last save").
- **The digest check, spiked** (2026-10-23, the build-or-adopt audit;
  [build-or-adopt.md](build-or-adopt.md), entry 93):
  - cms 0.3.0-rc.0, der 0.8 (`ber`), sha1 and sha2, pinned exactly:
    about 40 lines at the core. The stable cms 0.2.3 can't read the 138
    of the corpus's 593 signatures that are BER.
  - 546 of the 593 are checkable. It agrees with pyHanko on
    `0008830.pdf` and `0010094.pdf`: neither is intact.
  - Unknown algorithms, no signed attributes (28) and parse failures
    keep asking; only a clear mismatch counts as broken.
  - Hash lazily, once per source, cancellable: N signatures cost N
    hashes of nearly the whole file.
  - The fix went further than the spike: with no signed attributes, an
    RSA PKCS#1 v1.5 value is opened to its DigestInfo, as 0008830 and
    0010094 need; other algorithms still keep asking.

### B6: sheaf-app's clippy fails without default features

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR:
  `Contributions::add_panel` is built only with the `plugins` feature,
  as only a plugin's manifest declares a panel. `cargo clippy -p
  sheaf-app --all-targets --no-default-features -- -D warnings` is clean;
  still no CI job builds it.
- **Found:** 2026-10-22, M10.5 slice 18 (pre-existing)
- **Where:** `crates/sheaf-app/src/contrib.rs:158`,
  `Contributions::add_panel`
- **Severity:** annoyance (build)
- **What happens:** `cargo clippy -p sheaf-app --all-targets
  --no-default-features -- -D warnings` fails: `add_panel` is dead
  without the `plugins` feature.
- **Expected:** clean, or the configuration isn't supported.
- **Repro:** the command above.
- **Notes:** no CI configuration builds it. Gate the fn on the feature,
  or decide the no-plugins build isn't supported and say so.

### B7: Clear OCR text silently does nothing on a damaged old layer

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR:
  `PageBlocks::legacy_ocr_stays` says a layer stream is outside the
  pair. On a page with nothing else to take out, a Clear (or a Set)
  adds no filter, changes nothing, and the commit says the layer stays
  (`Notice::OcrStays`, the save's Note's words); a page with text taken
  out keeps the save's Note, so neither says it twice. Tests:
  `a_lone_legacy_layer_without_its_q_stays_and_says_so`
  (materialize/ocr_tests.rs) and, through the app's command (F8),
  `removing_recognized_text_says_what_stays`.
- **Found:** 2026-10-22, M10.5 slice 18
- **Where:** `ClearOcrText`'s lowering (`crates/sheaf-doc/src/tx/`), the
  legacy `/SheafOcrLayer` selector (`StreamSel::SheafOcr`)
- **Severity:** annoyance
- **What happens:** an older sheaf's OCR layer is a `q` plus a
  `/SheafOcrLayer` stream. If another tool lost the `q` (no pair, no
  block), a Clear adds no filter: it changes nothing and says nothing.
- **Expected:** a Note that the old layer stays, as other kept artifacts
  get ("… stays on page 3: taking it out would change what else the
  page draws").
- **Repro:** a page whose only legacy OCR stream has no leading `q`;
  ClearOcrText; save.
- **Notes:** taking it out without the `q` isn't safe (`3 Tr` would
  leak), so keeping it is right; only the silence is wrong.

### B8: The comment summary PDF has no title, producer or creation date

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR: the summary's
  primary is `memory::titled`'s (`Session::new_titled`), a PDF of no
  pages whose `/Info` has the title, `/Producer` sheaf and
  `/CreationDate`, which a save keeps. No op sets document info: that's
  for the Properties editor's op (Pending) to design. Test: sheaf-cli's
  `the_summary_has_its_title_producer_and_date` (`-o`, both layouts).
- **Found:** 2026-10-22, Claude, during M10.5 slice 19 (step 5, the
  summary as a Session)
- **Where:** sheaf-ops `comments::summary`, the CLI's `sheaf comments
  summary` and the app's Comments panel
- **Severity:** annoyance
- **What happens:** the summary is now a Session made with `new_empty`
  plus an insert of the `Memory(Summary)` blob. A session's output takes
  its `/Info` from its primary, which is the empty document, and no op
  sets document info. So the saved summary has no `/Title`, `/Producer`
  or `/CreationDate`.
- **Expected:** as before slice 19: `/Title` is the summary's title,
  `/Producer` is sheaf, `/CreationDate` is when it was made. A viewer's
  title bar and Properties show them.
- **Repro:** `sheaf comments summary in.pdf -o out.pdf`, then
  `qpdf --show-object=trailer out.pdf` (or Properties): no `/Info`
  entries.
- **Notes:** Greg: "log it" (2026-10-22). The fix is an op (or a Session
  option) that sets document info, which a Properties editor would also
  want. Until then the summary could set it on its `new_empty` primary.

### B12: Flatten draws a comment's first appearance state when it has no `/AS`

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR: a non-widget
  annotation whose `/AP /N` has states draws what PDFium shows, the page
  view's engine: `/AS`'s state, else the one `/V` (or its parent's)
  names, else `/Off`, else nothing; never its first, and nothing made of
  its border or fill. A name or a string names a state. Widgets keep
  their rule (M10 bug 10). It's one choice (`source::appearance`'s
  `Shows::form`), so Export Flattened, print with markup, print preview
  and page images all follow it. pdf.js differs where the two disagree:
  it never takes `/V` or `/Off`, and for a Square, Circle, Ink, the text
  markups and FreeText with a border or a fill it draws an appearance of
  its own. Tests: `a_comment_with_states_draws_the_one_pdfium_shows`
  (materialize/flatten_tests.rs: states and no `/AS`, with a border, with
  a fill, an `/Off`, a `/V`, a parent's `/V`, a `/V` naming none, an
  `/AS` naming none, a string `/AS`), `flatten_takes_what_shows_and_leaves_the_rest`
  and `printable_leaves_what_isnt_printed` (their comment draws its
  `/Off`), sheaf-cli's `a_comment_with_states_exports_as_the_page_view_shows_it`,
  and sheaf-render's `the_page_view_shows_a_comment_s_value_s_state_or_its_off`,
  which pins what PDFium shows of the same file.
- **Found:** 2026-10-22, M10.5 slice 19 (step 2, `FlattenAnnotations`)
  (pre-existing: the old flatten did the same)
- **Where:** `FlattenAnnotations`' choice of appearance for a
  non-widget (`crates/sheaf-doc/src/tx/flatten.rs`)
- **Severity:** wrong output
- **What happens:** a comment (not a widget) whose `/AP /N` has states
  and no `/AS` is flattened with its first state. PDFium shows nothing
  for it (it falls back to `/V`, then `/Off`), so flattening draws what
  no viewer showed.
- **Expected:** flatten draws what the viewer showed, nothing here.
  Widgets already have no first-state fallback.
- **Repro:** a Square annotation with `/AP << /N << /A … /B … >> >>` and
  no `/AS`; Export Flattened.
- **Notes:** rare in the wild; check pdf.js and Acrobat agree with PDFium
  before changing it.
  - **Checked (2026-10-23, the backlog-fixes PR):** pdf.js and PDFium
    agree on nothing for the repro, but not on the cases beside it.
    Neither ever draws the first state.
    - **pdf.js** (6.4.299, `Annotation.setAppearance` in
      src/core/annotation.js): with `/N` a dictionary and no `/AS` name,
      no appearance at all, never `/V` or `/Off`. Square, Circle, Line,
      Polygon, Ink, the text markups and FreeText then draw one pdf.js
      makes from `/Rect`, `/C`, `/IC` and the border, if the border has
      width or there's a fill; a Stamp draws nothing.
    - **PDFium** (sheaf's, the page view's engine): `/AS`, else `/V` (the
      annotation's or its parent's), else `/Off`; nothing when that
      state isn't there, and no appearance of its own while `/AP /N` is.
    - **Hand-made files** (one Square, states A and B, no `/AS`): both
      draw nothing; with `/BS /W 4` or an `/IC`, PDFium nothing and
      pdf.js its own square; with states A and `/Off` instead, PDFium
      draws Off and pdf.js nothing (its own square with a border). A
      Circle is the Square's case; a Stamp draws nothing in both.
    - **Acrobat:** not found in its docs. ISO 32000-1 12.5.5 requires
      `/AS` when `/AP` has subdictionaries, so such a file isn't
      conforming.
    - **Not only Export Flattened:** print with markup, print preview
      and Export Pages' images draw through the same flatten
      (`FlattenSet::Printable`), so they show the first state where the
      page view shows nothing (`sheaf export` drew state A on the repro).
    - **The choice:** PDFium's, what the page view shows (the
      coordinator, under Greg's standing instruction to take the
      recommended answer), over drawing nothing as pdf.js does but for
      its own appearances.

### B13: A print file with markup renumbers its header when reopened in sheaf

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR: print with
  markup leaves sheaf's private data out as print without markup does
  (`RemovePrivateData`, `Private::Scrub`), so what sheaf drew is plain
  content and nothing in the file is live. Chosen over writing an
  `AsDocument` keep's generator blocks as plain content: one rule for
  both kinds of print, through what's already there, and a print file
  has no use for the rest of sheaf's private data either (its marks are
  drawn into its pages). Test:
  `a_print_file_opened_again_says_the_document_s_numbers`
  (materialize/header_tests.rs: pages 5–6 of 10, of a document saved
  and not, the file and a save of it reopened say "Page 5 of 10").
- **Found:** 2026-10-23, M10.5 slice 19's wrap-up (the print numbering
  fix)
- **Where:** `Export::print` with markup (`crates/sheaf-doc/src/export.rs`),
  which keeps sheaf's private data (`Private::Keep`)
- **Severity:** wrong output
- **What happens:** printing pages 5–6 of 10 with markup writes "Page 5
  of 10", but the file keeps the header live in `/SheafGenerators`. Open
  the print preview's Save as PDF output in sheaf and the header is
  numbered against that file's own pages, "Page 1 of 2", and a save of
  it writes that. Other viewers show "Page 5 of 10". Print without
  markup writes the header as plain content, so it stays.
- **Expected:** the file reads "Page 5 of 10" in sheaf too.
- **Repro:** a 10-page document with a "Page <<1>> of <<N>>" header;
  print preview of pages 5–6 with markup, Save as PDF; open it in sheaf.
- **Notes:** before the fix the print file said "Page 1 of 2"
  everywhere. Candidate fixes: print with markup leaves out sheaf's
  private data as print without markup does (Scrub), or an `AsDocument`
  keep writes its generators' blocks as plain content.

### B21: `layers::LayerId` is a lopdf `ObjectId` under another name

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR: `LayerId` is
  sheaf's own, a type over the group's object that only layers.rs reads
  (not an `ObjRef`: the facts are read from a `&Source`, with no `Src`
  to hold, and the view's override is keyed by source already).
  `tests/object_ids.rs` sees through a type alias that names `ObjectId`,
  or names such an alias, and catches a path through one; its probe
  `an_alias_of_an_object_id_is_seen_through` tests that. With the alias
  back, `only_a_source_s_readers_and_writers_name_an_object_id` fails
  on realize/mod.rs and realize/inputs.rs.
- **Found:** 2026-10-23, M10.5 slice 20
- **Where:** `crates/sheaf-doc/src/layers.rs` (`pub type LayerId =
  ObjectId`), used by `ViewInputs::off` and the app's layers panel
- **Severity:** annoyance (a design leak)
- **What happens:** an optional content layer is named by its file
  object id outside the `ObjectId` allowlist. `tests/object_ids.rs`
  can't see through the alias.
- **Expected:** a layer id of sheaf's own, as pages and marks have, or
  an `ObjRef`.
- **Notes:** also make the grep test catch type aliases of `ObjectId`.

### B22: `FlatOutlineItem` derives Serialize and Deserialize that nothing uses

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR: the derives
  and their `serde` attributes are gone; nothing read or wrote it.
- **Found:** 2026-10-23, M10.5 slice 20 (pre-existing)
- **Where:** `crates/sheaf-doc/src/open/outline.rs`
- **Severity:** annoyance (dead code)
- **What happens:** the derives have no serializer anywhere in the
  workspace, and they pin the type's shape for no reader.
- **Expected:** drop them, or say who reads it.

### F8: A command to remove recognized text

- **Closed:** 2026-10-23. Done in the backlog-fixes PR: Document >
  Remove Recognized Text… (`docview::RemoveRecognizedText`,
  `ocr_clear.rs`) asks whether the selected pages or every page, then
  runs `ClearOcrText` as a planner, one undo step, the notice its line,
  what stays (B7), or that there was none. The command line's form is
  `sheaf edit`'s `clear-ocr-text`, which README now says. Tests:
  `removing_recognized_text_is_one_undo_step`,
  `removing_recognized_text_says_what_stays` and
  `it_asks_about_the_selected_pages_or_every_page`.
- **Found:** 2026-10-22, M10.5 slice 18
- **What:** an app command (and a CLI form) that runs `ClearOcrText`
  on the selected pages or all pages, with the usual Notes.
- **Why:** slice 18 made saved OCR layers removable (bug 14), but only
  tests, the API and fuzz call `ClearOcrText`. A user who wants a bad
  OCR layer gone can only re-OCR over it.
- **Overlap:** the OCR dialog (`crates/sheaf-app/src/ocr_*`); M10.5
  slice 19's `sheaf edit` takes any op as JSON, so the CLI form may come
  free.
- **Notes:** small: a command and a confirmation. The op, its Notes
  and the Rewrite it plans exist.

### F14: An automated test of `sheaf scan --ocr`

- **Closed:** 2026-10-23. Fixed in M5 slice 1 (`4481573`):
  `crates/sheaf-cli/tests/scan.rs`. It runs in `cargo test --workspace`
  on every CI runner (sheaf-cli's default features build Tesseract, and
  CI fetches the OCR data); without them the scan fails, never skips.
- **Found:** 2026-10-22, M10.5 slice 19 (step 5, scan as a Session)
- **What:** a CLI test that runs `sheaf scan --ocr` on a few images and
  checks the output's text layer: the words found, one OCR block a page,
  one font.
- **Why:** slice 19 moved scan onto a Session (an empty session plus an
  insert of the images, then OCR). Only a hand run checks the `--ocr`
  path; the OCR op itself is tested in sheaf-doc and the corpus tiers.
- **Overlap:** M5's scanner intake (ROADMAP), which reworks scan.
- **Notes:** the worry was that Tesseract may be missing in CI. Check
  what `--ocr` needs (sheaf-ocr's default engine vs the `tesseract`
  feature, used only for orientation) and either run it everywhere or
  install what it needs in CI. Don't make it skip silently.

### B11: Create from Scans still writes a file and reopens it

- **Closed:** 2026-10-23. Fixed in M5 slice 2: Create from Scans streams
  its pages into an untitled document in a new or empty window; nothing
  is written until Save, which is Save As.
- **Found:** 2026-10-22, M10.5 slice 19
- **Where:** `Workspace::create_from_scans`
  (`crates/sheaf-app/src/workspace.rs`), `open_paths`
- **Severity:** annoyance
- **What happens:** scan builds a Session now, but the app writes its
  result to a file and opens that file, where Combine hands its
  Document to a window as made.
- **Expected:** the scan's Session handed to a window, as Combine does:
  no file until the user saves.
- **Repro:** Create from Scans in the app.
- **Notes:** M5's scanner intake (ROADMAP) reworks this path; do it
  there.

### F22: Check what sheaf writes against the Arlington PDF Model

- **Closed:** 2026-10-24. Done in the Arlington checks PR: the model's
  `tsv/latest` at `d148fee` (2026-10-21) is vendored in
  `vendor/arlington-pdf-model` with its LICENSE, NOTICE.txt and a
  SOURCE.md, and read by the test-only `sheaf_doc::arlington` (behind
  `fixtures`): the TSVs; their predicates, 19 functions implemented
  (4,515 of the model's 4,729 calls), `fn:Deprecated` marking what it
  wraps and still allowing it, any other counted as a check not made;
  and a walk from the trailer as written that follows the Link column,
  each object's table chosen by TestGrammar's scoring (ported,
  Apache-2.0). Its rules: types, required entries (inherited ones too),
  too new for the file's version, deprecated, allowed values, direct and
  indirect, SpecialCase, unresolved, unreached, malformed trees, and in
  an object sheaf built whole, a key the model doesn't list that's no
  second- or third-class name nor sheaf's private key; every unlisted key
  is counted by its kind of name. `Hold::written` holds only what a save
  or export wrote (the objects an Append's cross-reference section lists,
  read or not; the objects a Rewrite derived; never a copy of a file's),
  each built whole or merged by its key; `Report::beyond` carries a
  finding in a merged object that the file read has too, in any of its
  objects (per object is B46), and
  `crates/sheaf-ops/tests/arlington_allowed.tsv` holds the rest for a
  backlog entry (B40–B45), by file and by version. Tests: sheaf-doc's
  `arlington::` units (54, each rule fault-checked);
  `crates/sheaf-ops/tests/arlington.rs`'s 38 default tests, every
  save-side writer both ways (`every_mark_kind_holds` to
  `a_pdf_2_0_file_holds`), the exports, Combine, the summary, images, a
  scan and the stamps, and the allowances (each `*` row needed, each
  entry open, B43 only from 2.0); and
  `what_sheaf_writes_on_every_form_holds_to_arlington` (`just
  tier-arlington`, in `premerge`): 4,389 of 4,392 forms (3 don't open),
  2,062,906 objects as 127 tables, 11,345,111 keys, 150,604 checks not
  made, 18,670 findings carried from the forms, 1,289 allowed, none
  left, in a minute; its form and save counts are asserted. Two writer
  fixes: a form `stamp_lib::page_forms` makes of a page without
  resources gets an empty `/Resources` (library stamps, page stamps,
  page watermarks and the summary's page pictures), and a file of no
  pages has no content stream.
- **Found:** 2026-10-23, the build-or-adopt audit
- **What:** a test-only checker that reads the Arlington PDF Model's TSV
  files
  ([pdf-association/arlington-pdf-model](https://github.com/pdf-association/arlington-pdf-model),
  Apache-2.0) and checks every object sheaf builds: annotations,
  appearance streams' resource dictionaries, form fields, the catalog
  and trailer keys it writes, and later StructElems and output intents.
  It checks keys, types, required entries and version predicates. Run it
  over the emission in the existing save tiers.
- **Why:** PDF interoperability is required, and viewers are lenient, so
  a malformed key can pass PDFium and pdf.js and still fail Acrobat or a
  validator. krilla runs Arlington in CI for the same reason.
- **Overlap:** F9 (`conformance/`); M14 PDF/A; M17 PDF/UA; the interop
  tests with qpdf and pdf.js.
- **Notes:** no Rust implementation exists; TestGrammar (C++) and
  veraPDF's Arlington service do. The TSVs are data: vendor a pinned
  copy under the test fixtures with its license, or read them from
  `../pdf-corpus/tools`. Start with the objects sheaf creates, not every
  object it copies. [build-or-adopt.md](build-or-adopt.md), entry 94.

## Sources checked

Outside material already compared against sheaf, so the comparison isn't
done twice.

- **2026-10-21: Gemini's summary of a PDFlyer demo video**
  ([share link](https://share.gemini.google/5pTedPNwgmKO)). It covers
  the calculator, sticky notes, status stamps, connectors and Note
  Summary. It produced B1 (closed) and F1–F3. F4 and F5 came out of
  discussing it.
  - **Already in sheaf from M9:**
    - placing, moving and re-editing tapes, and tape titles;
    - sticky notes;
    - Set Status and Reset Status, with Reset keeping the status stamp;
    - connector groups;
    - Note Summary and Stamp Summary as Comments presets, whose rows
      jump to the mark and select it.
  - **Secondhand, and partly at odds with CCH's help:**
    - In the report, Note Summary includes stamps. In CCH's help it
      covers notes and tapes, and stamps have their own Stamp Summary.
    - In the report, clicking a connector jumps to its target. In CCH's
      help, connectors are label groups navigated from the right-click
      menu.

    Prefer `docs/prior-art/pdflyer.md`, which cites CCH's help.
- **2026-10-22: OCR on pages that already have text.** Adobe's
  [renderable text](https://helpx.adobe.com/acrobat/kb/error-could-perform-recognition-acrobat.html)
  note, ocrmypdf's
  [advanced docs](https://ocrmypdf.readthedocs.io/en/stable/advanced.html)
  (skip, redo, force) and ABBYY's
  [incorrect text layer](https://support.abbyy.com/hc/en-us/articles/360020818320-Recognition-of-the-document-that-has-incorrect-text-layer)
  article. Foxit's documentation didn't say. It produced F10.
- **2026-10-23: QR codes to links.** Neither Acrobat nor Foxit appears
  to do it. Foxit
  [turns URLs in text into links](https://resource-cms.foxit.com/tutorials/how-add-hyperlinks-pdfs-seamless-navigation-and-interactivity),
  and an Adobe forum
  [question asking for this](https://community.adobe.com/t5/acrobat/make-clickable-hyperlinks-from-qr-or-zappar-codes-in-pdf-how/m-p/9981646)
  has no answer. It produced F16.
- **2026-10-23: an appearance with states and no `/AS`.** pdf.js's
  `src/core/annotation.js` (6.4.299) and PDFium, on hand-made files, and
  Acrobat's documentation, which doesn't say. It's in B12's notes.
- **2026-10-23: the build-or-adopt audit** ([build-or-adopt.md](build-or-adopt.md)): crates.io, docs.rs
  and GitHub for every need in the milestones and the backlog, by four
  researchers. It produced B24–B29 and F18–F23, and lines on B5, F9 and
  F13.
