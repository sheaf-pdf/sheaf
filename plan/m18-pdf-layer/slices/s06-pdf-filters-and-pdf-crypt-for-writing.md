# M18 slice 6: pdf-filters and pdf-crypt, for writing

Status: open · Part of [M18: the PDF layer](../README.md) · in pdf-lint's workspace, by pdf-lint's agent

**What:** the codecs encode as well as decode, by Greg's ruling that
the family exposes writing ([sheaf.md](../sheaf.md#pdf-filters)).

- **pdf-filters:** Flate encoding, and the PNG predictors' encoding (Up,
  which sheaf's cross-reference streams use), and any other filter
  sheaf writes today.
- **Both Flate backends** (Greg, 2026-10-27): zlib-rs and miniz_oxide,
  one API, for decoding and encoding. pdf-lint probably takes
  miniz_oxide, and sheaf zlib-rs, for speed: every sheaf build uses
  zlib-rs ([D4](../../research/build-or-adopt/decisions.md#d4-one-flate-backend-for-the-workspace),
  ruled 2026-10-27), so the same input gives the same bytes.
  - **Additive features, the backend named by the caller** (rec.):
    each feature compiles its backend in, and a call names the one it
    wants (a `Backend` value that exists only with its feature).
  - **Not mutually exclusive** (Greg asked): Cargo unifies features
    across a build, and sheaf's `cargo test --workspace` builds
    pdf-lint (miniz_oxide) with sheaf (zlib-rs), so a `compile_error!`
    on both would break sheaf's tests.
  - **Not a precedence rule either,** as flate2's features are (zlib-rs
    wins when both are on): that's how sheaf-app's `zip` turned on
    `flate2/zlib-rs` and changed the shipped app's bytes unseen (D4).
    Named by the caller, a dependency's feature can't change anyone's
    output.
- **Room for sheaf to be fast** ([crates.md](../crates.md#decisions-across-the-family),
  decision 9): decoding and encoding into a buffer the caller gives,
  and no copy of data that needs no decoding.
- **pdf-crypt** ([sheaf.md](../sheaf.md#pdf-crypt)):
  - `encrypt_string` and `encrypt_stream` without the `writer` feature,
    the IV still the caller's;
  - a dictionary builder that takes its random bytes (salts, and
    AES-256's file key) from the caller, for M11's new passwords;
  - `writer::dictionary`, its salts fixed, stays the tests'.

**From sheaf** ([from-sheaf.md](../from-sheaf.md#the-rest-of-the-writer-slices-3-6-and-7)):
why the IV stays the caller's (`serialize/crypt.rs`), and the PNG Up
predictor sheaf's cross-reference streams use (`serialize/xref.rs`).

**Where:** `crates/pdf-filters/`, `crates/pdf-crypt/`.

**Checked:**
- round trips: encode, then decode, gives the input; encrypt, then
  decrypt, likewise, every revision and method;
- every test on both backends; on each, the same bytes as sheaf's
  compression today on that backend, for sheaf's settings
  ([slice 12](s12-sheaf-writes-through-pdf-structure.md) holds sheaf's
  outputs to it);
- throughput on each backend beside flate2's, on sheaf's corpus streams;
- the builder's dictionaries open in qpdf, pdf.js and PDFium;
- fuzz bodies for the encoders.
