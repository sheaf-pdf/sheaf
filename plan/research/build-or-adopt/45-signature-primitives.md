# Build or adopt 45: Signature primitives (verify; sign with in-memory keys)

Area: M12: Signatures · Part of [build or adopt](README.md)

- **Need:** verify RSA PKCS#1 v1.5 (with SHA-1 and 1024-bit keys, for old
  files), RSA-PSS, ECDSA P-256, P-384 and P-521, and Ed25519; sign with
  PKCS#12 keys, RSA and ECDSA. Platform-held keys are entries 49–51.
- **Where:** M12 slices 2–3; B5 (only for signatures with no signed
  attributes). D13.
- **Today's plan:** none.
- **Candidates:**
  - **RustCrypto** (pure Rust, `no_std`):
    [rsa](https://crates.io/crates/rsa) stable 0.9.10, 0.10.0-rc.19
    (2026-10-06), MIT OR Apache-2.0.
    **[RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html)
    (Marvin, timing) is unpatched in both** ("Still affected as of
    2026-09-12 … Local use on a non-compromised computer is fine");
    verifying isn't affected, since it uses no private key.
    [p256](https://crates.io/crates/p256) and
    [p384](https://crates.io/crates/p384) 0.14.0 (2026-07);
    [ecdsa](https://crates.io/crates/ecdsa) 0.17.0 (2026-07-02);
    [ed25519-dalek](https://crates.io/crates/ed25519-dalek) 3.0.0
    (2026-07-06; BSD-3-Clause); [sha1](https://crates.io/crates/sha1)
    0.11.0 and [sha1-checked](https://crates.io/crates/sha1-checked)
    (collision detection; 0.10.0, 0.11.0-rc.0).
  - **[ring](https://crates.io/crates/ring)** 0.17.14 (2025-03-11;
    Apache-2.0 AND ISC; Brian Smith; C and assembly). **Already in
    sheaf-app's normal build** through wasmtime-wasi-http's rustls. RSA
    PKCS#1 and PSS signing from PKCS#8 or DER keys; verifies with
    `RSA_PKCS1_1024_8192_SHA1_FOR_LEGACY_USE_ONLY`, P-256, P-384 (no
    P-521) and Ed25519. No RSA decryption.
  - **[aws-lc-rs](https://crates.io/crates/aws-lc-rs)** 1.18.2
    (2026-10-12; ISC AND (Apache-2.0 OR ISC); AWS; a C build through
    aws-lc-sys): adds PKCS#1 v1.5 RSA decryption
    (`Pkcs1PrivateDecryptingKey`) and legacy SHA-1 RSA. In the lockfile
    only, not in a normal build.
- **Fit:** RustCrypto verifies every algorithm in the corpus but GOST;
  use sha1-checked for SHA-1 digests, given the SHAttered PDFs. For
  PKCS#12 signing, the rsa crate is timing-unsafe in theory: acceptable
  on a desktop with no remote oracle, but an audit tool would flag it.
  ring is constant-time and already built.
- **Recommendation:** **adopt RustCrypto for verification** behind a
  verify function sheaf owns. Medium. For PKCS#12 signing, prefer
  **ring** in a `signature::Signer` adapter, so cms's builder takes it,
  over the rsa crate while Marvin is open. Decide at kickoff.
