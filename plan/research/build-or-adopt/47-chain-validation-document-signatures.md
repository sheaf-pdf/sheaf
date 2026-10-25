# Build or adopt 47: Chain validation for document signatures

Area: M12: Signatures · Part of [build or adopt](README.md)

- **Need:** build a path from the signer's certificate (the CMS's
  certificates, the DSS, optionally an AIA fetch) to a trust anchor;
  validate it at the signing time (the timestamp's, or the claimed time,
  flagged as such); check KeyUsage (digitalSignature or nonRepudiation);
  accept a document-signing EKU or none (anyExtendedKeyUsage,
  emailProtection, Adobe's Authentic Documents 1.2.840.113583.1.1.5,
  Microsoft's Document Signing 1.3.6.1.4.1.311.10.3.12, RFC 9336's
  id-kp-documentSigning 1.3.6.1.5.5.7.3.36); check revocation from the
  DSS, CMS archives or online; bound the work on hostile certificate
  bags.
- **Where:** M12 slice 3 (the verify panel) and slice 5 (LTV).
- **Candidates:**
  - **[rustls-webpki](https://crates.io/crates/rustls-webpki)**
    0.103.16 (2026-10-20; ISC; rustls' ctz and djc; 895M downloads;
    `no_std`, no `unsafe`; needs ring or aws-lc-rs for algorithms).
    `verify_for_usage(algs, anchors, intermediates, time, usage,
    revocation, …)` (`src/end_entity.rs:103-122`) takes any `UnixTime`.
    Its EKU check is a trait (`ExtendedKeyUsageValidator`) with
    `KeyUsage::required` and `required_if_present` for any OID. It checks
    CRLs (`RevocationOptions`) and budgets path building. Gaps: no OCSP;
    no certificate-policy processing; no end-entity KeyUsage-bit check
    found in its API (unverified); built-in algorithms need RSA ≥ 2048
    and have no SHA-1, though callers can pass their own
    `SignatureVerificationAlgorithm`; its hard-coded `DISTRUSTED_SPKIS`
    are TLS CAs (`verify_cert.rs:917`).
  - [synta-x509-verification](https://crates.io/crates/synta-x509-verification)
    0.5.1 (2026-10-17; MIT OR Apache-2.0; FreeIPA's abbra; 2k downloads):
    RFC 5280 §6 with pluggable crypto. Young.
  - [pkix-path](https://crates.io/crates/pkix-path) 0.3.2 (2026-06-15;
    Apache-2.0 OR MIT; Mark Atwood; 14k downloads). Young; provenance not
    checked.
  - tsp-ltv 0.6.0 (entry 52): chain building, trust stores, KeyUsage/EKU
    role checks. openssl's `X509_verify_cert` (entry 44's costs).
  - Prior art: [pyhanko-certvalidator](https://pypi.org/project/pyhanko-certvalidator/)
    0.33.0 (2026-10-17; MIT).
- **Fit:** none validates document-signing chains with PDF semantics out
  of the box. webpki is the most used and best-tested RFC 5280 core, and
  its usage and time hooks let sheaf supply the PDF policy.
- **Recommendation:** **build on rustls-webpki, behind a trait;** sheaf
  adds the EKU policy, the signing time, revocation from DSS and CMS, and
  the legacy algorithms. Medium. Re-check synta at kickoff.
