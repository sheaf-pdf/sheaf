# Build or adopt 46: X.509 parsing

Area: M12: Signatures · Part of [build or adopt](README.md)

- **Need:** parse signer, CA, TSA and OCSP-responder certificates, and
  CRLs, from CMS, `/DSS`, PKCS#12 files and the OS stores; read names,
  extensions, KeyUsage, EKU, AIA and CRL distribution points; tolerant
  enough for real files, and bounded.
- **Where:** M12 slices 2–5.
- **Candidates:** [x509-cert](https://crates.io/crates/x509-cert) 0.3.0
  (2026-07-09; Apache-2.0 OR MIT; RustCrypto; 49M downloads; `no_std`,
  `forbid(unsafe_code)`; a `crl` module and a builder);
  [x509-parser](https://crates.io/crates/x509-parser) 0.18.1 (2026-02-05;
  MIT OR Apache-2.0; rusticata's chifflier alone; 193M downloads;
  nom-based, a `verify` feature on ring; p12-keystore depends on it);
  [rasn-pkix](https://crates.io/crates/rasn-pkix) 0.29.0.
- **Fit:** x509-cert is what cms decodes into; the spike decoded every
  certificate in 592 SignedData. p12-keystore puts x509-parser in the
  tree as a second parser.
- **Recommendation:** **adopt x509-cert.** High.
