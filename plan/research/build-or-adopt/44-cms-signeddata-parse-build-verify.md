# Build or adopt 44: CMS SignedData: parse, build, verify

Area: M12: Signatures · Part of [build or adopt](README.md)

- **Need:** parse `/Contents` in BER or DER, ignoring the zero padding.
  Build detached SignedData for adbe.pkcs7.detached and
  ETSI.CAdES.detached from an external digest (signed attributes:
  contentType, messageDigest, signingCertificateV2, optionally Adobe's
  revocation archive; unsigned: the signature timestamp), with keys sheaf
  can't see (tokens, CNG, the Keychain). Verify the digest against
  messageDigest, the signature over the DER signed attributes, and the
  ESS signing-certificate binding.
- **Where:** M12 slices 2, 3 and 5; B5. D2.
- **Today's plan:** no crate named. "Digital signatures: sign with
  PKCS#12 files, the Windows certificate store, and PKCS#11 tokens"
  (`ROADMAP.md:397-398`); "Signature panel: validation status,
  certificate details, changes since signing" (`:399-400`).
- **Candidates:**

  | Crate | Latest | License | Maintenance | Build | Notes |
  |---|---|---|---|---|---|
  | [cms](https://crates.io/crates/cms) ([RustCrypto formats](https://github.com/RustCrypto/formats), 343★) | stable **0.2.3, 2024-01-08** (der 0.7); **0.3.0-rc.0, 2026-10-13** (der 0.8) | Apache-2.0 OR MIT | tarcieri plus the org; 15M downloads | Pure Rust, `no_std`, `forbid(unsafe_code)` | Types for SignedData, EnvelopedData (KeyTrans, KeyAgree, KEM), TimeStampedData; builders for SignedData (`SignerInfoBuilder` with an external digest, taking `signature::Signer` keys) and EnvelopedData. **No verification API.** 0.3 decodes BER (`der` feature `ber`) |
  | [openssl](https://crates.io/crates/openssl) | 0.10.81, 2026-06-12 | Apache-2.0 (OpenSSL 3 is Apache-2.0) | sfackler, alex; 435M downloads | **FFI to OpenSSL**; a vendored C build on Windows and macOS | CMS sign and verify, PKCS#12, OCSP, an X509 store. A C parser in the path of untrusted input, and a heavy cross-platform build |
  | [rasn-cms](https://crates.io/crates/rasn-cms) | 0.29.0, 2026-10-05 | MIT OR Apache-2.0 | XAMPPRocky alone; 4.5M downloads | Pure Rust; rasn codecs include BER | Types only, in its own ecosystem, apart from x509-cert |
  | [cryptographic-message-syntax](https://crates.io/crates/cryptographic-message-syntax) | 0.28.0, 2025-08-17 | **MPL-2.0** | indygreg; 6M downloads; last release 14 months ago | Pure Rust (bcder, ring) | Sign, verify and a TSP client, built for Apple code signing. MPL-2.0 is a cost |

- **The B5 spike over the corpus:** cms 0.2.3 failed "indefinite length
  disallowed" on **138 of 593**, and one more on a duplicate in a SET OF
  (454 parsed). cms 0.3.0-rc.0 decoded the whole SignedData, with every
  certificate through x509-cert 0.3, for all but one blob, which uses a
  `CertificateChoices` `[1]` alternative (an old attribute certificate)
  cms 0.3 doesn't define (`cms-0.3.0-rc.0/src/cert.rs:30-35`; F20).
- **Fit:**
  - cms 0.3 is the only pure-Rust option that reads real-world PDF CMS
    and builds SignedData with keys held elsewhere.
  - Verification is sheaf's own: compare digests, check the signature
    over the signed attributes (entry 45's primitives), check the ESS
    certificate hash.
  - **Timing:** the 0.3 line isn't finished. x509-ocsp 0.3.0-pre and
    x509-tsp 0.2.0-pre are on master, unpublished
    ([x509-ocsp](https://github.com/RustCrypto/formats/blob/master/x509-ocsp/Cargo.toml),
    [x509-tsp](https://github.com/RustCrypto/formats/blob/master/x509-tsp/Cargo.toml)).
    pkcs12 0.2.0-pre.0 pins `cms = "=0.3.0-pre.1"`, so **p12-keystore
    0.4.1 and cms 0.3.0-rc.0 don't resolve together** (`cargo
    generate-lockfile` fails).
- **Recommendation:** **build on cms 0.3, der 0.8 and x509-cert 0.3.**
  Medium-high: the design is clear, but the crates are release
  candidates. Pin exactly until the round is published; send upstream the
  missing `CertificateChoices` alternatives; re-check the line at
  kickoff.
