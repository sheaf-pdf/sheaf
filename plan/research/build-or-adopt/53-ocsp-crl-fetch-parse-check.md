# Build or adopt 53: OCSP and CRL: fetch, parse, check

Area: M12: Signatures · Part of [build or adopt](README.md)

- **Need:** parse OCSP responses and CRLs from the DSS, from Adobe's
  `adbe-revocationInfoArchival` signed attribute (144 corpus signatures
  carry one) and from online fetches; build OCSP requests and verify
  responders; fetch from AIA and CRL distribution points under caps (CRLs
  can be megabytes).
- **Where:** M12 slices 3 and 5 ("LTV (embed revocation info)",
  `ROADMAP.md:401`).
- **Candidates:** [x509-ocsp](https://crates.io/crates/x509-ocsp) 0.2.1
  (2024-01-08; Apache-2.0 OR MIT; tarcieri): types and a builder on der
  0.7; 0.3.0-pre on master, unpublished. x509-cert's
  `crl::CertificateList`. rustls-webpki's CRL checks (no OCSP).
  [rasn-ocsp](https://crates.io/crates/rasn-ocsp) 0.29.0. tsp-ltv (entry
  52).
- **Recommendation:** **build on x509-ocsp 0.3 and x509-cert's types**,
  with sheaf's HTTP client and caps; tsp-ltv as prior art. Medium.
