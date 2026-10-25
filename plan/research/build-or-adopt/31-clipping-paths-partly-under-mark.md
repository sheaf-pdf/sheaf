# Build or adopt 31: Clipping paths partly under a mark (polygon booleans)

Area: M11: Security and redaction · Part of [build or adopt](README.md)

The security section's detail; core's note that both crates are MIT OR
Apache-2.0 settles the plan's license check.
- **Need:** keep what's outside the marks of a painted path partly under
  them: a fill under its rule (nonzero or even-odd), a stroke whose ink
  must not survive under the mark, or both. The result is written back as
  path operators; a clip path can't be used instead. Only paths that
  intersect a mark are rewritten, and every other byte is kept. Input is
  untrusted: coordinates can be NaN, ±1e38, or millions of segments.
- **Where:** M11 slice 6; slice 9's "content outside the crop box". D6.
- **Today's plan:** "Partly covered paths are clipped geometrically,
  keeping what's outside the mark. Curves are flattened only in the
  paths that get clipped, with a polygon-boolean crate such as `geo` or
  `i_overlay` (licenses to check)" (`m11.md:160-163`); "Path clipping and
  the crate for it" (`:500`).
- **Candidates:**

  | Crate | Latest | License | Maintenance | Build | Fit notes |
  |---|---|---|---|---|---|
  | [i_overlay](https://crates.io/crates/i_overlay) ([GitHub](https://github.com/iShape-Rust/iOverlay), 215★) | 9.1.0, 2026-10-14 | MIT OR Apache-2.0; its i_float and i_shape are MIT | Nail Sharipov alone; 132 versions since 2023-08; majors 8→9 in 2026-09; 10.4M downloads; powers geo's booleans | Pure Rust, `no_std`; `unsafe` only for `get_unchecked` (77 sites); rayon optional | Union, intersection, difference, xor; fill rules EvenOdd, NonZero, Positive, Negative; f32/f64 APIs over an integer grid; polyline clip with `ClipRule { invert, boundary_included }`; stroke and outline offsetting. Its README: "Infallible APIs panic on invalid bounds" (non-finite values, or past 2^500 for f64); the fixed-scale APIs return errors |
  | [geo](https://crates.io/crates/geo) | 0.33.1, 2026-04-20 | MIT OR Apache-2.0 | georust; active | Pure Rust | `BooleanOps` **pins i_overlay `>=4.5.1, <4.6.0`**, five majors behind; brings geo-types, rstar, robust, spade, earcut and more. A GIS API for no gain |
  | [clipper2-rust](https://crates.io/crates/clipper2-rust) ([GitHub](https://github.com/larsbrubaker/clipper2-rust), 28★) | 1.2.0, 2026-09-18 | **BSL-1.0** (Boost: permissive) | Lars Brubaker; created 2026-02; 155k downloads | Pure Rust port of Angus Johnson's Clipper2; 1 `unsafe`; only num-traits; a WebAssembly demo | `difference_d` with fill rules, open (stroke centerline) subjects, `rect_clip`, offsetting. Claims "exact behavioral match with C++" over 457 tests |
  | [clipper2](https://crates.io/crates/clipper2) | 0.6.0, 2026-05-06 | MIT OR Apache-2.0 | tirithen | FFI to C++ Clipper2 | A C++ build; the pure port is better |
  | [geo-clipper](https://crates.io/crates/geo-clipper) | 0.9.0, 2025-02-08 | ISC | lelongg | FFI to Clipper 1 | Old engine; no |
  | [linesweeper](https://crates.io/crates/linesweeper) | 0.5.0, 2026-10-05 | MIT OR Apache-2.0 | Joe Neeman; 99k downloads; its [GitHub repo](https://github.com/jneem/linesweeper) is **archived** (last push 2026-01-25), yet releases continue (where it's developed is unverified) | Pure Rust, no `unsafe`, on kurbo | Booleans on Bézier paths **without flattening**, EvenOdd and NonZero; "Currently in an early beta state" |
  | [flo_curves](https://crates.io/crates/flo_curves) | 0.8.1, 2026-08-25 | Apache-2.0 | Logicalshift; 376k downloads | Pure Rust | Bézier booleans; not evaluated further |
  | lyon | 1.0.19 | MIT OR Apache-2.0 | nical | Pure Rust | Tessellation; no booleans |

- **Fit:**
  - **Fills:** a difference under the original fill rule handles
    self-intersecting, multi-subpath PDF fills. The output is simple
    contours with holes, so it can be written back with nonzero `f`
    whatever the original rule; confirm in the spike.
  - **Strokes:** no crate clips "a stroke's ink" directly. (a) Expand
    the stroke to its outline with kurbo's `stroke` (joins, caps, miter
    limit, dashes; `kurbo-0.13.1/src/stroke.rs:260`), take the
    difference and fill it: exact, but the stroke becomes a fill, which
    M13's editing would see. (b) Clip the centerline with i_overlay's
    inverted string clip, or clipper2's open subjects, against marks
    grown by half the line width under the CTM and the miter limit: the
    stroke stays a stroke, with new butt ends at the cuts. A choice for
    kickoff (D6).
  - **Hostile input:** validate coordinates (finite, bounded) and cap the
    segment count before the float API, or use the fixed-scale APIs,
    because the infallible ones panic.
  - **Churn:** four i_overlay majors this year, so pin it exactly.
- **Recommendation:** **adopt i_overlay directly, behind a one-function
  sheaf-doc module** (path minus marks), with kurbo for flattening and
  stroke expansion. Medium. Run clipper2-rust beside it in the corpus
  tier: the two kept areas must agree within tolerance. Watch
  linesweeper (curves kept as curves, but beta and archived). Not geo.
