# M10 slice 18: The AF functions in Rust

Status: open · Part of [M10: Forms](../README.md)

**The AF functions in Rust** (proposal 12).
- **On the edit API:** pure Rust, in the host's resolve stage (M10.5
  §6.3). They produce display and calculated values plus their
  decision reads, which the commit validates.
- `AFNumber`, `AFPercent`, `AFDate` (`Ex` and the indexed formats),
  `AFTime`, `AFSpecial` (ZIP, ZIP+4, phone, SSN) and the
  `KeystrokeEx` masks, `AFRange_Validate`, `AFSimple_Calculate`,
  `AFMakeNumber`, `AFMergeChange`, and the `util` calls they need.
- Recognizing a one-call script with literal arguments.
- Wired in: keystrokes filtered through slice 11's hook; validation
  on commit, with sheaf's dialog; format output in the appearance;
  `AFSimple_Calculate` in `/CO` order.
- Rounding as `toFixed` does.
- Data-driven tests from pdf.js's unit tests, PDFium's expected
  outputs and the corpus's argument sets (`AFNumber_Format(0,1,0,0,
  "",true)` ×278, the `KeystrokeEx` masks, the `AFDate` formats from
  `mmddyyyy` to `mm/dd`), credited in the notice. PDFium's and the
  corpus's vectors, and the second spike's 12 added cases, cover what
  pdf.js doesn't test (`AFTime_*`, `AFPercent_Keystroke`,
  `AFSpecial_Format`, `AFMergeChange`). The spike's 153
  `util.printf` and 51 `toFixed` vectors, computed from IEEE doubles
  and ECMA-262's rules, check the rounding.
- The state forms' JSON replay files (proposal 21).
- **Checks:** NY IT-201's amounts show formatted with scripts off;
  VA's empty `AFSimple_Calculate` gives 0;
  `AFDate_KeystrokeEx("mmddyyyy")` takes `10142026`, and
  `AFDate_KeystrokeEx("mm/dd/yyyy")` refuses `13/45/2026`;
  `AFTime_Format(1)` shows a 24-hour `13:05` as 1:05 pm; the replays
  pass for the forms the Rust path covers.
