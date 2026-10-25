# Build or adopt 50: Windows certificate store and CNG signing

Area: M12: Signatures · Part of [build or adopt](README.md)

- **Need:** list the user's `MY` certificates with private keys, filter
  by KeyUsage and EKU, offer a picker; sign a hash through NCrypt
  (smart cards behind CNG minidrivers, with Windows asking for the PIN);
  read ROOT and other stores for verification.
- **Where:** M12 slice 4 ("the Windows certificate store",
  `ROADMAP.md:397`).
- **Candidates:** **[windows](https://crates.io/crates/windows)** 0.62.2
  (2025-10-06; MIT OR Apache-2.0; Microsoft's kennykerr; 375M downloads):
  FFI bindings; sheaf-instance uses 0.61. CertOpenStore,
  CryptAcquireCertificatePrivateKey, NCryptSignHash, NCryptDecrypt,
  CryptUIDlgSelectCertificateFromStore.
  [schannel](https://crates.io/crates/schannel) 0.1.30 (2026-10-16; MIT;
  sfackler and steffengy; 340M downloads; windows-sys 0.61):
  `CertStore::open_current_user`, `CertContext::private_key().acquire()`
  gives an `NcryptKey` handle, but **no signing call**: it's for TLS.
- **Recommendation:** **build on the windows crate** (on GPUI's 0.62, as
  entry 7 says); schannel optional for listing. Medium-high.
