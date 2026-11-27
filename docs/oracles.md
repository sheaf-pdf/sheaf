# Oracles

The independent implementations sheaf's reading and writing are checked
against: what each is trusted for, which version, how it runs, and the
tests that use it. Started 2026-10-23; what to adopt, and when, was
decided with Greg the same day.

Rules for an oracle:

- **More trustworthy than what it checks.** A mature implementation with
  a track record, never a fresh crate (see [Not oracles](#not-oracles)).
- **Evidence, not the definition of correct.** Where oracles disagree, or
  one has a quirk (PDFium renders some files differently when it loads
  them a second time), the spec and the oracles together decide. A
  difference from PDFium or Acrobat isn't a bug by itself.
- **Copyleft runs as a separate process, in tests only.** GPL, LGPL and
  AGPL tools (poppler, MuPDF, Ghostscript, veraPDF, EU DSS) may be run
  and their behavior studied, never linked, copied or translated, and
  never shipped. Their test files stay outside the repo, as in
  `../pdf-corpus`. Permissive code (PDFium, pdf.js, qpdf) may also be
  read and quoted.
- **Pinned and named.** Each row says the version the tests ran
  against. A tier that needs a tool says so when it's missing, never
  passing in silence (`interop.rs` prints "skipped qpdf" / "skipped
  pdf.js"; corpus tiers panic on a wrong corpus layout).

## In use

| Oracle | Trusted for | Version | How it runs | Where |
|---|---|---|---|---|
| **PDFium** | Rendering; character boxes and text; page counts; what a page reads as; how an annotation reads (its rect, color, texts, flags, the state it shows) and what it draws with no appearance | `chromium/8125`, PDFium 158.0.8125.0 (pdfium-binaries; `scripts/fetch-pdfium.sh` checks the archive's sha256), through pdfium-render 0.9.6, pinned exactly (`=0.9.6`, feature `pdfium_8125`). Against chromium/7881, sheaf's build until M18 slice 26 (`just render-dump` and every tier, before and after), it opens, counts and sizes alike all 6,041 files the dump reads, and moves pixels on 3,401 of their 54,004 pages (most at glyph and hairline edges; some unembedded fonts' substitutes drawn heavier; a widget whose `/Rect` is empty drawn into the page's corner), text on 53 (order and spaces in right-to-left and complex scripts, U+00A0 kept) and the appearances its form filler makes (the fonts it adds by charset drawn in the machine's Noto Sans Mono Bold, 0xA0 for a space), for which tier-fields' classes were re-held; every other tier and the 7k gates hold what they held. The corpus manifests' `pdfium` and `pdfium_pages` columns are another build, PDFium 156.0.8076.0 through pypdfium2 5.14.0, and nothing reads them (`pages`, which sheaf-doc's tests read, is 8076's count, else qpdf's) | Linked: it's also sheaf's renderer. BSD-3 | `realize_corpus`'s pixel tiers, `tier-lexer` (`content_lexer.rs`: a page's tokens as sheaf reads them through pdf-content-streams, with its join and color spaces, by kind against PDFium's page objects over the pool; with pdf-content-streams' own PDFium cases, it holds to PDFium the tokens blocks are found and splices judged by, which the `splice` fuzz target, with no second reader since sheaf's lexer went, holds only to themselves), the interpreter tiers (`content_interp.rs`: glyph boxes, write back and render); annotations: pdf-annots reads them under PDFium's rules, held by its own PDFium cases, and a change to what sheaf reads, flattens or draws of them is set beside PDFium with `just annots-dump` before and after, each difference asked of PDFium (chromium/8152 for M18 slice 24, which also rendered every mark kind's appearance before and after at 4 px/pt with sheaf's own PDFium build; the family's tests hold the looks since, and sheaf's goldens the bytes); a text field's appearance (M10 slice 6): PDFium's own, made by its form filler for the same value (the field given its `/V` and no `/AP`, its page's widgets drawn once by `FPDF_FFLDraw`, which makes the appearance each lacks, and a copy saved with `FPDF_SaveAsCopy`; a copy saved before that drawing holds none), drawn appearances only by the same path as sheaf's save and set beside it inside the widget's box, by sheaf's linked build (chromium/8125), on Linux alone; pdf-forms was held to chromium/8152, and the fixtures' PDFium-side appearances are the same, token for token (each number as its f32), under 7881, 8125 and 8152, and 8125 draws them as 7881 did, byte for byte: sheaf-ops' `fields.rs` (fixtures) and `just tier-fields` (every corpus form, in `premerge`) |
| **poppler** | That sheaf's output draws in Linux desktops' viewers (Evince, Okular): where `pdftoppm` puts its ink | Whatever is installed for the tests (26.01.0 here) | Separate process (`SHEAF_PDFTOPPM`, else `pdftoppm` on the `PATH`). GPL: never linked or shipped | `interop.rs`'s ignored `text_field_appearances_pass_qpdf_and_poppler_draws_them_where_pdfium_s` (M10 slice 6): each pixel case's output inks the box PDFium's copy inks, and nothing outside it |
| **qpdf** | File structure: `--check`, cross-reference tables, filters, encryption; repairs; whether a linearization holds (`--check-linearization`) | Whatever is installed for the tests (12.3.2 here); 12.4.2 for the corpus manifests | Separate process (`SHEAF_QPDF`, else `/usr/bin/qpdf`). Apache-2.0 | `crates/sheaf-ops/tests/interop.rs`, serialize tests, the corpus manifests' `qpdf` columns; `rewrite_size.rs` (`just tier-size`: `--check` on each Save As no worse than on its source, and `--object-streams=disable` and `=generate` sizes as the reference); append_corpus's `linearization_agrees_with_qpdf_before_and_after_an_append` (`--check-linearization` on the pool sample, before and after an Append) |
| **pdf.js** | Whether Firefox reads sheaf's output; its appearance rules (read as prior art, e.g. backlog B12) | pdfjs-dist 6.4.299 | Separate process under node (`SHEAF_PDFJS`). Apache-2.0 | `interop.rs` |
| **pyHanko** | Signature integrity: what a signature covers and whether its digest holds | 0.37.0 | Offline, through `uv` in `../pdf-corpus/tools/inventory.py`, which writes the `signatures/` manifest. MIT | `forms::sigs::integrity`'s tier `the_corpus_s_signatures_agree_with_pyhanko` (B5) |
| **pikepdf, pypdf** | The corpus inventories' structure facts | pikepdf 10.17.0 | Offline, through `uv` in the corpus tools. MPL-2.0, BSD-3 | `../pdf-corpus` manifests |
| **Acrobat** | What users see; Preflight | Current | By hand | Hand checks in slice notes; `docs/prior-art/acrobat.md` |
| **pdf-lint**, on the Arlington PDF Model | The objects sheaf writes, against ISO 32000-2's grammar and content/structure rules: each key's types, required entries (inherited ones too), the version a key, type or value came in, deprecations, allowed values, direct and indirect references, object streams (`ObjectStream` and `packed`), content streams, fonts, color spaces, trees, page trees, XMP and linearization. Never the files sheaf reads, most of which break a rule somewhere | pdf-lint 0.2.0-rc.1 ([sheaf-pdf/pdf-lint](https://github.com/sheaf-pdf/pdf-lint)), which embeds the PDF Association's model with its corrections and additions | Separate process (`SHEAF_PDF_LINT`, else `~/.local/share/sheaf-tools/pdf-lint/pdf-lint` via `just setup-pdf-lint`, else `pdf-lint` on the `PATH`; in CI, `sheaf-pdf/setup-pdf-lint@v1`). Apache-2.0; never linked or shipped. Its reader (`pdf-structure`) reads each file sheaf wrote from its bytes, decoding object streams, cross-reference streams and content streams and decrypting what's encrypted natively (no qpdf), so what a writer gets wrong below the objects is found too (a `/Length` that doesn't end at `endstream`, a key written twice). It walks a written file from its trailer as TestGrammar does; a predicate it doesn't implement is counted as a check not made, never guessed. It holds only what a save or export wrote (`lint::written`, from the save's record); a finding in an object sheaf merged over a file's dictionary that the file has too, in any of its objects, is carried (`Report::beyond`; per object is backlog B46), and `Rule::Updated` on an `Append` onto a linearized file is expected (ISO 32000-2 F.1, Table F.1). What the version deprecates is a finding, and a field merged with its widget is checked as both tables. A key the model doesn't list fails only in an object sheaf built whole, when it's no second- or third-class name (sheaf's own keys are `SHEF_`). Anything else fails unless `crates/sheaf-ops/tests/arlington_allowed.tsv` holds it for a backlog entry, by file and PDF version; each of its rows must be needed, and name an open entry | `crates/sheaf-ops/tests/arlington.rs`: every writer on documents built in code, each save both ways; `just tier-arlington` over every corpus form, in `premerge`, its counts of forms and saves asserted |

pdf-lint is new code, so it is held to the model's own cells and to
TestGrammar's behavior (Apache-2.0, studied and ported with attribution)
by the tests in its own repository, which began as sheaf's checker's.
Like TestGrammar, it checks an object as the table it fits best when
nothing in it says which, so tables that differ only in what's absent
can't be told apart. Unlike TestGrammar, which checks each object as the
first table that reaches it, it checks a field merged with its widget as
both tables at once, the field's by its `/FT` and `/Ff` (no `/Ff` is a
check box, as ISO 32000-2 has it), so a field's keys on its widget are
checked, not counted as unlisted. Where the model and the spec differ,
the spec decides: pdf-lint corrects the cells it knows of (its
`model/corrections.tsv`, which closed B45), and for the rest the
allowance says so (B40).

## Adopting next

Greg, 2026-10-23: small PRs, each run in the local pre-commit as
`interop.rs` is, and on CI's Ubuntu runner where a package installs
there.

| Oracle | Would check | License, how |
|---|---|---|
| **poppler** beyond `pdftoppm`'s text field cases (In use: every output, and `pdftotext`) and **MuPDF** (`mutool draw`, `mutool show`) in `interop.rs` | That sheaf's output opens and renders without errors in two more viewer families: Linux desktops (Evince, Okular) and MuPDF-based viewers | GPL and AGPL, separate processes; `poppler-utils`, `mupdf-tools`. Linux |
| **libtiff** (`tiffinfo`, `tiffcp`) | sheaf's CCITT G4 output decoded independently: TIFF export, and G4 images in PDFs (it would have caught B24) | libtiff licence (BSD-style), separate process; `libtiff-tools` |
| **veraPDF**, on a Java runtime | That sheaf's saves of a file that passes PDF/A or PDF/UA still pass, or say why (proposal 39's Note); the `conformance/` tier (backlog F9). The same runtime serves EU DSS at M12 | GPLv3+ or MPLv2+, separate process; `default-jre-headless` |

## By milestone

Written into each milestone's plan (`plan/m10-forms/README.md`, `plan/m11-security/README.md`, and
ROADMAP's M12, M14 and M17), to be applied at its kickoff.

| Milestone | Oracle | Would check | License, how |
|---|---|---|---|
| **M10** | pdf.js's scripting | The AF functions (`AFNumber_Format` and the rest), on the same inputs | Apache-2.0, node |
| | PDFium's form drawing (FFLDraw) | Generated check box and choice appearances (slice 8); text fields' are in use since slice 6 (In use, PDFium) | Linked already |
| | OTS (OpenType Sanitizer) and fontTools | The fallback font sheaf embeds (slice 7): well formed as browsers require, and what the subset holds | BSD-3 and MIT, separate processes |
| **M11** | PDFium, pdf.js's text layer and poppler's `pdftotext` at once, plus a byte search of `qpdf --qdf` output | That redacted text is gone, whichever reader looks | As above |
| **M12** | pyHanko (signing and validation) | PAdES levels B-B to B-LTA of what sheaf signs, and its DSS | MIT, separate process |
| | `openssl cms -verify` | The CMS of each signature sheaf makes | Apache-2.0, separate process |
| | EU DSS | The EU's reference PAdES validation (ETSI EN 319 102-1) | LGPL, Java, separate process |
| **M14** | veraPDF | PDF/A conversion and validation | As above |
| | OpenJPEG (`opj_decompress`) | JPX images sheaf re-encodes, if it does | BSD-2, separate process |
| | `qpdf --check-linearization` | Fast Web View: in use since B3 (In use, above); later what the linearization crate writes (between milestones, item 4) | As above |
| **M17** | veraPDF | PDF/UA-1 and UA-2 | As above |
| | PAC; NVDA, Narrator, VoiceOver, Orca | Matterhorn checks beside veraPDF's; the app with a screen reader | By hand (PAC is Windows freeware) |

Also planned:

| Oracle | For | Where |
|---|---|---|
| **hayro-interpret** | A second interpreter and renderer beside PDFium, to tell PDFium's quirks from sheaf's bugs | Backlog F17 (the content interpreter PR). Pure Rust, MIT/Apache, test-only (M10.5 D55) |
| **hayro-syntax** | A second reader in the lazy-reader corpus test | Backlog F23 |
| **zxing-cpp** | A second barcode reader | M5 slice 3's trait; with M5's deferred slices. Apache-2.0 |

Real scanners (the office's Fujitsu fi-6130s) are the oracle for M5's
deferred slices: CI sees no scanner driver.

## Considered and dropped

- **Ghostscript:** veraPDF validates PDF/A; Ghostscript would add only
  its own conversion behavior.
- **underskrift** (a Rust PAdES signer and validator): it would bring an
  older lopdf and RustCrypto line into the dev tree, and pyHanko with EU
  DSS covers what it would check.

## Not oracles

- **The `pades` crate** (0.1.4, first published 2026-10-23): one account
  published 22 crates across unrelated domains in two days from one
  monorepo, and it implements its own SHA-2, RSA, DER and CMS. Neither a
  dependency nor an oracle (Greg, 2026-10-23).
- **Code from GPL, LGPL or AGPL projects,** as code: MuPDF's and
  Okular's are studied for behavior only (`plan/m10-forms/README.md`, `plan/m11-security/README.md`).
