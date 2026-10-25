# Build or adopt 48: PKCS#12 files

Area: M12: Signatures · Part of [build or adopt](README.md)

- **Need:** open .p12 and .pfx files exported by Windows, macOS, Java and
  OpenSSL: legacy PBE (SHA-1 with 3DES or RC2-40) and PBES2 (PBKDF2 with
  AES). Find the key and its chain.
- **Where:** M12 slice 2 ("sign with PKCS#12 files", `ROADMAP.md:397`).
- **Candidates:**
  - **[p12-keystore](https://crates.io/crates/p12-keystore)** 0.4.1
    (2026-10-13; MIT/Apache-2.0; ancwrd1 alone; 17 releases since
    2024-03; 9.6M downloads; 25★; pure Rust on RustCrypto; no `unsafe`).
    Reads and writes "modern and legacy encryption schemes"; no MD5-based
    schemes or unencrypted stores (README). **It depends on
    pre-releases** (cms 0.3.0-pre.1, and pkcs12 0.2.0-pre.0, which pins
    cms `=0.3.0-pre.1`) and on x509-parser 0.18.
  - [pkcs12](https://crates.io/crates/pkcs12) (RustCrypto) 0.1.0 and
    0.2.0-pre.0: types and the PKCS#12 KDF only.
  - [p12](https://crates.io/crates/p12) 0.6.3: last release 2022-02-18.
    Stale. And openssl's `Pkcs12`.
- **Fit:** p12-keystore covers it, but can't share a build with cms
  0.3.0-rc.0 today (entry 44).
- **Recommendation:** **adopt p12-keystore once it moves to the published
  cms 0.3.** Until then, M12 pins the pre-release set it needs. Medium.
