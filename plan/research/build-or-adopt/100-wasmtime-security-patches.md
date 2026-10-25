# Build or adopt 100: wasmtime security patches (plugins and the forms guest)

Area: Infrastructure that's built (confirmed) · Part of [build or adopt](README.md)

- sheaf locks wasmtime, wasmtime-wasi and wasmtime-wasi-http 49.0.2.
  49.0.3 (2026-10-14) and 49.0.4 (2026-10-19) fix GHSA-mghf-wjfp-p5xm
  (critical; see [Already acted on](audit-notes.md#already-acted-on-2026-10-23)), and
  several wasi-http issues. The core section also cited GHSA-5qpp
  (unverified, as above).
- GHSA-w36r (a host panic on long URLs) is in the `http` crate, which
  sheaf locks at 1.5.0, after the fix; whether that's mitigated is
  unverified.
- wasmtime 50.0.0 is out (2026-10-20). The advisories:
  [bytecodealliance/wasmtime](https://github.com/bytecodealliance/wasmtime/security/advisories).
- **Recommendation:** the patch bump to 49.0.4, now: PR #33. Check the
  guest's compile-cache key, which hashes wasmtime's version.
