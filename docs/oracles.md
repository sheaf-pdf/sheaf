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
| **PDFium** | Rendering; character boxes and text; page counts; what a page reads as | `chromium/7881` (pdfium-binaries, through pdfium-render 0.9.4); the corpus manifests' `pdfium` columns are PDFium 156.0.8076.0 through pypdfium2 5.14.0 | Linked: it's also sheaf's renderer. BSD-3 | `realize_corpus`'s pixel tiers, the content lexer tier, the interpreter tiers (`content_interp.rs`: glyph boxes, write back and render) |
| **qpdf** | File structure: `--check`, cross-reference tables, filters, encryption; repairs | Whatever is installed for `interop.rs` (12.3.2 here); 12.4.2 for the corpus manifests | Separate process (`SHEAF_QPDF`, else `/usr/bin/qpdf`). Apache-2.0 | `crates/sheaf-ops/tests/interop.rs`, serialize tests, the corpus manifests' `qpdf` columns |
| **pdf.js** | Whether Firefox reads sheaf's output; its appearance rules (read as prior art, e.g. backlog B12) | pdfjs-dist 6.4.299 | Separate process under node (`SHEAF_PDFJS`). Apache-2.0 | `interop.rs` |
| **pyHanko** | Signature integrity: what a signature covers and whether its digest holds | 0.37.0 | Offline, through `uv` in `../pdf-corpus/tools/inventory.py`, which writes the `signatures/` manifest. MIT | `forms::sigs::integrity`'s tier `the_corpus_s_signatures_agree_with_pyhanko` (B5) |
| **pikepdf, pypdf** | The corpus inventories' structure facts | pikepdf 10.17.0 | Offline, through `uv` in the corpus tools. MPL-2.0, BSD-3 | `../pdf-corpus` manifests |
| **Acrobat** | What users see; Preflight | Current | By hand | Hand checks in slice notes; `docs/prior-art/acrobat.md` |
| **The Arlington PDF Model** | The objects sheaf writes, against ISO 32000-2's grammar: each key's types, required entries (inherited ones too), the version a key, type or value came in, deprecations, allowed values, direct and indirect references. Never the files sheaf reads, most of which break a rule somewhere | PDF Association's `d148fee` (2026-10-21), `tsv/latest`, vendored in `vendor/arlington-pdf-model` (backlog F22) | In process, test-only: `sheaf_doc::arlington`, behind `fixtures`, reads the TSVs and walks a written file from its trailer as TestGrammar does. A predicate it doesn't implement is counted as a check not made, never guessed. It holds only what a save or export wrote; a finding in an object sheaf merged over a file's dictionary that the file has too, in any of its objects, is carried (per object is backlog B46). A key the model doesn't list fails only in an object sheaf built whole, when it's no second- or third-class name nor sheaf's private key. Anything else fails unless `crates/sheaf-ops/tests/arlington_allowed.tsv` holds it for a backlog entry, by file and PDF version; each of its rows must be needed, and name an open entry. Apache-2.0 data | `crates/sheaf-ops/tests/arlington.rs`: every writer on documents built in code, each save both ways; `just tier-arlington` over every corpus form, in `premerge`, its counts of forms and saves asserted |

The Arlington reader is new code, so it is held to the model's own cells
and to TestGrammar's behavior (Apache-2.0, studied and ported with
attribution) by its unit tests. Like TestGrammar, it checks an object as
the table it fits best, so tables that differ only in what's absent
can't be told apart: a button field without `/Ff` fits every button table
and is checked as a push button (the model gives `/Ff` no default; ISO
32000-1's 0 makes it a check box). Where the model and the spec differ,
the spec decides, and the allowance says so.

## Adopting next

Greg, 2026-10-23: small PRs, each run in the local pre-commit as
`interop.rs` is, and on CI's Ubuntu runner where a package installs
there.

| Oracle | Would check | License, how |
|---|---|---|
| **poppler** (`pdftoppm`, `pdftotext`) and **MuPDF** (`mutool draw`, `mutool show`) in `interop.rs` | That sheaf's output opens and renders without errors in two more viewer families: Linux desktops (Evince, Okular) and MuPDF-based viewers | GPL and AGPL, separate processes; `poppler-utils`, `mupdf-tools`. Linux |
| **libtiff** (`tiffinfo`, `tiffcp`) | sheaf's CCITT G4 output decoded independently: TIFF export, and G4 images in PDFs (it would have caught B24) | libtiff licence (BSD-style), separate process; `libtiff-tools` |
| **veraPDF**, on a Java runtime | That sheaf's saves of a file that passes PDF/A or PDF/UA still pass, or say why (proposal 39's Note); the `conformance/` tier (backlog F9). The same runtime serves EU DSS at M12 | GPLv3+ or MPLv2+, separate process; `default-jre-headless` |

## By milestone

Written into each milestone's plan (`todos/m10.md`, `todos/m11.md`, and
ROADMAP's M12, M14 and M17), to be applied at its kickoff.

| Milestone | Oracle | Would check | License, how |
|---|---|---|---|
| **M10** | pdf.js's scripting | The AF functions (`AFNumber_Format` and the rest), on the same inputs | Apache-2.0, node |
| | PDFium's form drawing (FFLDraw) | Generated text, check box and choice appearances | Linked already |
| | OTS (OpenType Sanitizer) and fontTools | The fallback font sheaf embeds (slice 7): well formed as browsers require, and what the subset holds | BSD-3 and MIT, separate processes |
| **M11** | PDFium, pdf.js's text layer and poppler's `pdftotext` at once, plus a byte search of `qpdf --qdf` output | That redacted text is gone, whichever reader looks | As above |
| **M12** | pyHanko (signing and validation) | PAdES levels B-B to B-LTA of what sheaf signs, and its DSS | MIT, separate process |
| | `openssl cms -verify` | The CMS of each signature sheaf makes | Apache-2.0, separate process |
| | EU DSS | The EU's reference PAdES validation (ETSI EN 319 102-1) | LGPL, Java, separate process |
| **M14** | veraPDF | PDF/A conversion and validation | As above |
| | OpenJPEG (`opj_decompress`) | JPX images sheaf re-encodes, if it does | BSD-2, separate process |
| | `qpdf --check-linearization` | Fast Web View (already in M14's text) | As above |
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
  Okular's are studied for behavior only (`todos/m10.md`, `todos/m11.md`).
