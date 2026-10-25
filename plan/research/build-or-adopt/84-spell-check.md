# Build or adopt 84: Spell check

Area: Pending · Part of [build or adopt](README.md)

The fonts section's entry; core checked the same facts.
- **Need:** underline misspellings, with suggestions and "Add to
  dictionary", in `TextInput`/`TextArea`, notes and fields, with a tax
  word list.
- **Where:** `ROADMAP.md:839-865`. D15.
- **Today's plan:** engines listed (`:847-854`): the platform checkers;
  "spellbook (Helix's pure-Rust, Hunspell-compatible checker; MPL-2.0,
  alpha, no phonetic suggestions)"; "zspell (pure Rust); its license and
  suggestion API are unchecked."
- **Candidates:**
  - **[spellbook](https://crates.io/crates/spellbook) 0.4.2**
    (2026-06-03; MPL-2.0; helix-editor (the-mikedavis); 254k downloads;
    3 releases in 12 months; pushed 2026-07-11). The README says it
    "should be considered to be in _alpha_"; `check` "works well for
    `en_US`"; `suggest` "should behave the same as Nuspell's"; no
    phonetic suggestions; complex compounding "may work less well".
    `no_std`; only hashbrown (plus foldhash by default); about 10
    `unsafe` blocks (`get_unchecked` in the suggester and parser). Users:
    codebook (a Zed spell-check extension) and cargo-spellcheck.
    **MPL-2.0 is file-level copyleft:** static linking is allowed, but
    sheaf must offer the source of spellbook's files, with any changes. A
    cost, not a blocker.
  - **[zspell](https://crates.io/crates/zspell) 0.5.5** (2024-06-13):
    **Apache-2.0** (its LICENSE file; crates.io shows "non-standard").
    The checker is stable; suggestions are behind `unstable-suggestions`
    ("not yet finalized"). Last commit 2024-07-09; 150k downloads.
    Stale.
  - **[hunspell-rs](https://crates.io/crates/hunspell-rs) 0.4.0 (2022)
    and hunspell-sys 0.3.1 (2022):** FFI to C++ Hunspell (MPL-1.1/GPL-2/
    LGPL-2.1 tri-license). Stale.
  - **Platform:** Windows'
    [`ISpellChecker`](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Globalization/struct.ISpellChecker.html)
    (`windows` crate; Windows 8+) and macOS's
    [`NSSpellChecker`](https://docs.rs/objc2-app-kit/latest/objc2_app_kit/struct.NSSpellChecker.html)
    (`objc2-app-kit`).
  - **Dictionaries:** LibreOffice's en_US is SCOWL size 60 (2020.12.07),
    permissive: "Permission to use, copy, modify, distribute and sell
    these word lists … provided that the above copyright notice appears
    in all copies"; its affix file is under Geoff Kuenning's BSD license
    ([README_en_US](https://raw.githubusercontent.com/LibreOffice/dictionaries/master/en/README_en_US.txt)).
    Other languages vary (LGPL, GPL, MPL): load them from
    `/usr/share/hunspell` rather than bundling them.
- **Fit:** the platform checkers give users their own languages and word
  lists. spellbook is the only maintained pure-Rust engine with
  suggestions, for Linux, or as one engine everywhere. Nothing here
  touches PDF bytes.
- **Recommendation:** **adopt behind a trait:** platform checkers on
  Windows and macOS; spellbook on Linux, or everywhere if one behavior is
  wanted; bundle SCOWL en_US plus sheaf's tax word list; drop zspell.
  Medium. What decides it: whether firms want identical suggestions on
  every OS.
