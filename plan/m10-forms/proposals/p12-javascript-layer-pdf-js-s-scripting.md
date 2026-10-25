# M10 proposal 12: **The JavaScript layer is pdf.js's `scripting_api`, with the AF

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**The JavaScript layer is pdf.js's `scripting_api`, with the AF
functions also written in Rust.**
- **pdf.js's library** (Apache-2.0; 5,637 lines at `43c5e0d`) is
  Acrobat's object model: the event dispatcher, Field, Doc, `app`,
  `util`, the AF functions, numeric coercion, partial `getField`
  names, and the user-activation gate for dialogs. It talks to its
  host through one function with nine calls, and values cross as
  JSON.
  - It ran **unchanged in boa 0.22,** natively, with boa's default
    features plus `annex-b`: loading took 26.6 ms, setting up
    3.2 ms, and peak memory was 18 MB. A commit with keystroke,
    validate, two calculations and the format scripts took about
    0.6 ms. Its AF output matched the formats Acrobat's reference
    documents (`$1,234.50`, `($765.50)`, `123-45-6789`,
    `01/02/2026`), and formatting changed only the display.
  - It comes with tests: 58 unit tests covering
    `AFNumber_Format`/`_Keystroke`, `AFPercent_Format`,
    `AFDate_Format(Ex)`, `AFRange_Validate`, `AFSimple_Calculate`,
    `AFSpecial_Keystroke(Ex)`, `AFMakeNumber`, `AFExtractNums`,
    `AFParseDateEx`, `AFMakeArrayFromList`, `eMailValidate`,
    `AFExactMatch`, `printf`, `printd`, `printx`, `scand` and the
    events, and 70 integration tests on about 45 PDFs. None covers
    `AFTime_*`, `AFPercent_Keystroke`, `AFSpecial_Format` (18 uses in
    the corpus) or `AFMergeChange`. The 58 unit tests run in the
    guest, and the second spike's 12 added cases cover those four
    (below).
  - **Inside the wasm guest it runs unchanged too,** about 1.5–2x
    slower than the same code built natively, and within decision
    3's deadlines. The second spike built boa 0.22 with `annex-b`
    and `xsum` for wasm32-wasip2 and ran it under wasmtime 49.0.2
    with those limits. boa's file module loader refuses wasm,
    so a 40-line in-memory `ModuleLoader` serves pdf.js's 21 files
    (135,555 bytes of JavaScript), compiled in with `include_str!`.
    The guest is 6.64 MB; the time zone comes through a host import,
    and the guest needs 17 WASI interfaces, none of them files or
    network (decision 3). Medians:

    | | wasm | native |
    |---|---|---|
    | Load `scripting_api` (a new context; modules parsed, linked and run) | 33.4 ms | 22.1 ms |
    | Set up (`initSandbox`): NY IT-201, 214 fields | 62.5 ms | 30.3 ms |
    | Set up: GA 500 (591 fields) / OH IT 1040 (588) | 206 / 187.5 ms | 99.8 / 101.6 ms |
    | Document scripts at Open: NY / GA / OH | 6.8 / 69.9 / 62.0 ms | OH 39.0 ms |
    | A keystroke, taken or refused | 0.25 / 0.22 ms | |
    | A commit: keystroke, validate, 5 sums over 30 fields, formats | 5.77 ms | 3.88 ms |
    | Ohio's commit, its real 319-field `/CO` cascade | 70.1 ms | 43.6 ms |
    | A synthetic 300-field cascade, AF sum and format | 100.9 ms | 57.8 ms |
    | The same with custom `getField().value` scripts | 88.0 ms | 51.2 ms |

    - Set-up costs about 0.3 ms a widget. Building its payload with
      `serde_json` and `JsValue::from_json`, rather than parsing it
      as JavaScript, saves 20–30% (about 0.2 ms a widget: NY
      48.6 ms, GA and OH 139 ms).
    - A calculated field costs about 0.33 ms, since pdf.js
      re-evaluates each action's source (about 37 µs of compiling).
      `util.printf` and `AFNumber_Format` cost about 115 and 118 µs a
      call.
    - Peak linear memory was 65–77 MiB, of which 64 MiB is the
      mostly untouched stack: about 3 MiB for pdf.js and 2–12 MiB a
      form.
    - pdf.js's 58 unit tests ran in the guest unchanged but for one
      line (the sandbox import), with a jasmine stand-in in a second
      realm and a fresh context per sandbox, as pdf.js's testing mode
      does. The 12 added cases cover `AFSpecial_Format` (PDFium's
      vectors), `AFPercent_Keystroke`, `AFTime_*`, `AFMergeChange`,
      `util.printx`, the `AFNumber_Format` styles, `AFDate` round
      trips, `AFSimple_Calculate`'s exact sum and `Math.sumPrecise`,
      plus 153 `util.printf` and 51 `toFixed` vectors computed from
      IEEE doubles and ECMA-262's rules.
    - 65 of the 70 pass as is, and all 70 with the `Date.parse` shim
      below in the sandbox. The patches below are what the spike
      found.
- **No fallback needed.** Had pdf.js failed in the guest, M10 would
  have shipped the Rust AF path alone and moved custom scripts to a
  follow-up. The second spike shows it runs, so pdf.js ships in
  M10.
- **Writing our own instead** means the AF functions (about 600
  lines) plus about 3,000 lines of dispatcher and object model, and
  their tests. MuPDF's (AGPL) and Okular's (GPL) can't be used.
  Okular's also lacks `AFSpecial_KeystrokeEx` (used in NY, UT, IL and
  PA) and `AFRange_Validate` (Ohio's 116 fields, IL and CA DE-4).
  PDFium's (BSD-3) is C++ tied to its own form objects: its
  algorithms and test vectors only.
- **The Rust AF path:** a script that is a single AF call with
  literal arguments (`AFNumber_Format(2,0,0,0,"",true);`) runs
  natively, with no engine.
  - It covers 55% of the corpus's field scripts. Every format,
    keystroke, validate and calculate script in 7 of the 11 state
    forms is a single AF call. Five (IL, NJ, NY IT-201, NY IT-2105,
    VA) have no other scripts. CA 540 and CA 3519 also check dates on
    Blur in custom JavaScript, and CA 540 makes two pairs of check
    boxes exclusive through button actions; with scripts off those
    don't run.
  - In the wider survey, 84.5% (765 of 905) of the scripts other than
    Adobe's boilerplate were single AF calls.
  - So those forms format and check values with scripts off, in the
    CLI, and in builds without `forms-js`.
  - Two implementations can drift. A test runs both over the same
    cases (pdf.js's unit tests, the spike's added cases and vectors,
    PDFium's `public_methods_expected.txt`, the corpus's argument
    sets) and they must agree.
  - Rust's formatting rounds exact ties to even (`{:.0}` of 2.5 is
    `2`). The port rounds as JavaScript's `toFixed` does.
- **Licensing:** the vendored pdf.js files keep their Apache-2.0
  headers and get a "modified by sheaf" line where changed. A
  `packaging/licenses/NOTICE-forms-js.txt` follows `NOTICE-ocr.txt`,
  and ported tests are credited there too. It also carries the
  licenses of everything linked into the guest (boa under MIT, ICU4X
  under Unicode-3.0, regress and the rest), generated from the guest
  workspace's `cargo metadata` (its license fields). sheaf stays MIT OR Apache-2.0; those files
  stay Apache-2.0. PDFium's BSD-3 notice covers ported test vectors.
- **The patch set,** kept small over a pinned commit:
  - `valueAsString` returns the raw string; a rejected commit
    restores the previous value (proposal 13); the viewer's name and
    version; sheaf's console prefix; and passing `textSize`,
    `textFont`, `/TU`, the `/DA` color and alignment into the
    sandbox, which pdf.js leaves out.
  - **A Calculate event starts with the field's value**
    (`event.js`, `runCalculate`: `event.value = savedValue`, not
    `null`), as Acrobat does (to check; see Asks). Unpatched, every
    Ohio commit logs 39 `TypeError`s from `/CO` scripts such as
    `UPC_LName` that read `event.value.length`. Patched, none, and
    Ohio's commits take 43–49 ms. (GA 500 still logs one error of its
    own: `GN("S7_PORTION_NORMAL_LOSS")` names a field the file
    lacks.)
  - **Terminal fields indexed by dotted prefix** in `doc.js`'s
    `_getTerminalChildren` (about 25 lines). `Field.getArray()`
    builds its cache through it, and it scans every field, so a
    form's first calculation is O(n²). The first commit took
    194–212 ms on the synthetic form and 766 ms on the 300-field
    cascade; patched, 9–10 ms and 110–120 ms. Unpatched, a form with
    more than about 400 `AFSimple_Calculate` fields would likely
    pass the 1 s deadline on first use.
  - **A `Date.parse` shim,** run in each sandbox after pdf.js loads:
    a Proxy around `Date` for the legacy forms. boa 0.22's
    `Date.parse` takes only ISO 8601 and its own `toString` and
    `toUTCString` forms, and rejects "07/15/2007", "2000/01/05",
    "Apr 15 2007" and "Sun Apr 15 2007 03:14:15". Without it two
    `printd` tests fail, and so does `AFDate_Format`, through
    `_parseDate`'s fallback to `Date.parse`; two more fail only on
    the tests' own expected dates.
  - **Date and time checks as Acrobat does them.** pdf.js differs
    from Acrobat here in any engine: `AFTime_Format(1)` shows a
    24-hour "13:05" as "12:13 pm" (the format's regex misses, and
    `#tryToGuessDate` puts 13 in the minutes), and
    `AFDate_KeystrokeEx("mm/dd/yyyy")` takes "13/45/2026", since `new
    Date` rolls over and nothing checks the range. The Rust port
    follows Acrobat, so unpatched these are where the two disagree.
  - The Calculate, index and shim changes were made in the spike,
    and pdf.js's tests and the added cases still passed, 70 of 70.
  - Optional, if timings need them: building the set-up payload
    natively (above), and caching compiled action scripts.
