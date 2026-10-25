# Build or adopt 32: Curve flattening and stroke expansion

Area: M11: Security and redaction · Part of [build or adopt](README.md)

- **Need:** flatten Béziers to a tolerance in device terms (user-space
  tolerance over the CTM's scale); expand strokes with dashes and joins
  (entry 31's option a); affine transforms.
- **Where:** M11 slice 6.
- **Today's plan:** "Curves are flattened only in the paths that get
  clipped" (`m11.md:161-162`), no crate named.
- **Candidates:** [kurbo](https://crates.io/crates/kurbo) 0.13.1
  (2026-05-13; Apache-2.0 OR MIT; Linebender, 9 owners including Raph
  Levien; 57M downloads; 1,022★; pure Rust, `no_std` with libm), with
  `flatten` (`bezpath.rs:622`), `stroke`, `dash` and `Affine`, **already
  in the lockfile** through usvg 0.46;
  [lyon_geom](https://crates.io/crates/lyon_geom) 1.0.20 (2026-10-20; MIT
  OR Apache-2.0; nical; 13M downloads), with
  `CubicBezierSegment::flattened`, in the lockfile through gpui-pre's
  lyon.
- **Fit:** both flatten; only kurbo expands strokes, and linesweeper
  builds on kurbo.
- **Recommendation:** **adopt kurbo.** High.
