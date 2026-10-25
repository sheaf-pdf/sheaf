# Build or adopt 4: Flate: one backend in every build

Area: Cross-cutting: the PDF object layer, writing, and Flate · Part of [build or adopt](README.md)

The images section's detail; core agrees ("keep flate2 (zlib-rs)").
- **Need:** one compressor, so the same input gives the same bytes in
  every build.
- **Where:** every save (lopdf's `Stream::compress` at level 9,
  `vendor/lopdf/src/object.rs:814`); emit (`materialize/emit/mod.rs:860`);
  memory sources (`sheaf-doc/src/images.rs:138`), which rely on "the same
  inputs make the same bytes, so the same blob" (`sheaf-doc/src/memory.rs:5-6`).
  B26.
- **Today:** set by accident.
  - Every sheaf crate asks for `flate2 = "1"` with default features,
    which means miniz_oxide (`crates/sheaf-doc/Cargo.toml:34`,
    `crates/sheaf-ops/Cargo.toml:35`, lopdf).
  - sheaf-app's `plugins` feature brings `zip` with
    `deflate-flate2-zlib-rs` (`crates/sheaf-app/Cargo.toml:19,38`).
    Cargo unifies that into `flate2/zlib-rs`, and flate2 then picks
    zlib-rs (`flate2-1.1.10/src/ffi/mod.rs:69-77`). tesseract-rs's `zip`
    is a build dependency, so it doesn't count.
  - So: the shipped `sheaf` binary (which bundles the CLI) uses zlib-rs;
    the `sheaf-cli` binary, `cargo test -p sheaf-doc` or `-p sheaf-ops`,
    and `--no-default-features` app builds use miniz_oxide;
    `cargo test --workspace` uses zlib-rs.
- **Measured** (flate2 1.1.10, a synthetic 2550×330 gray scan):

  | Level | miniz_oxide | zlib-rs |
  |---|---|---|
  | 1 | 435,764 bytes | 643,987 bytes |
  | 6 | 411,964 bytes | 406,370 bytes |
  | 9 | 411,964 bytes | 411,261 bytes |

  The bytes differ at every level.
- **Fit:** the shipped app is consistent with itself. But a test that
  pins compressed bytes or sizes passes or fails depending on how it's
  run, a dev build without plugins writes different files, and neither
  backend promises the same output across its own versions.
- **Candidates:** [zlib-rs](https://crates.io/crates/zlib-rs) 0.6.8
  (2026-09-15; Zlib license; Trifecta Tech Foundation; about 456 `unsafe`
  in `src/`; 197M downloads); [miniz_oxide](https://crates.io/crates/miniz_oxide)
  0.9.1 (2026-03-13; MIT OR Zlib OR Apache-2.0).
- **Recommendation:** **adopt zlib-rs explicitly** in one workspace entry
  (`flate2 = { version = "1", default-features = false, features =
  ["zlib-rs"] }`), used by every crate. Check whether anything replays a
  memory source across builds or versions and expects the same BlobId.
  High on naming one backend; medium on which (zlib-rs is what ships;
  its own benchmarks say it's faster, unchecked). D4.
