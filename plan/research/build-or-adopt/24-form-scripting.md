# Build or adopt 24: Form scripting (slices 18–22)

Area: M10: Forms · Part of [build or adopt](README.md)

- **Need:** Acrobat's AF functions in Rust (slice 18), and pdf.js's
  `scripting_api` in boa, run as a wasm guest (slices 19–22).
- **Where:** `m10.md:81-92` (decision 1), `:104-…` (decision 3),
  `:1726-1892`; `ROADMAP.md:301-302`.
- **Today's plan:** "The form JavaScript engine is boa_engine"
  (`m10.md:81`); "boa runs as a WebAssembly guest in sheaf's wasmtime"
  (`:104`); "boa supports only its latest 0.x release" (`:265`).
- **What's changed:**
  - [boa_engine](https://crates.io/crates/boa_engine) is still 0.22.0
    (2026-08-28; Unlicense OR MIT). Its `main` raised the MSRV to 1.95 on
    2026-10-17; sheaf is on 1.98.
  - The issues that made boa unsafe in process are still open:
    [#3442](https://github.com/boa-dev/boa/issues/3442) (interrupt),
    [#4397](https://github.com/boa-dev/boa/issues/4397) and
    [#4311](https://github.com/boa-dev/boa/issues/4311) (parser stack
    overflow; updated 2026-10-21). The wasm decision holds.
  - [rquickjs](https://crates.io/crates/rquickjs) 0.14.0 (2026-09-18;
    MIT) binds QuickJS in C: in process it's C parsing hostile JS, and as
    a guest it needs wasi-sdk (as F12's qpdf does). Nothing favors it.
  - The AF port: on rustc 1.99, `format!("{:.2}", 0.125)` gives `0.12`
    where JavaScript's `(0.125).toFixed(2)` gives `0.13` (likewise 2.5
    to 0 places: `2` against `3`). The plan covers it: "The port rounds
    as JavaScript's `toFixed` does" (`m10.md:739-740`).
- **Recommendation:** unchanged. **Adopt boa 0.22 as a wasm guest,**
  rquickjs only if a guest gate fails, and **build the AF functions in
  Rust.** High.
