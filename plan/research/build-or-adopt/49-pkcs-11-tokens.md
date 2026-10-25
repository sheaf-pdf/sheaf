# Build or adopt 49: PKCS#11 tokens

Area: M12: Signatures · Part of [build or adopt](README.md)

- **Need:** load a token's module, list slots and certificates, log in
  with a PIN, sign (C_Sign) and later decrypt (C_Decrypt).
- **Where:** M12 slice 4 ("PKCS#11 tokens", `ROADMAP.md:398`).
- **Candidates:** **[cryptoki](https://crates.io/crates/cryptoki)**
  0.12.1 (2026-09-16; Apache-2.0; the Parsec team: wiktor-k, ionut-arm,
  hug-dev and the org; 3.3M downloads; 120★). It released 0.10.1, 0.11.1
  and 0.12.1 the same day, so older lines get fixes. **FFI** through
  libloading: the vendor's module loads at run time (119 `unsafe` sites).
  PINs held with secrecy. [pkcs11](https://crates.io/crates/pkcs11)
  0.5.0: last release 2020, stale.
- **Fit:** covers the API. Module discovery is sheaf's: p11-kit's proxy
  on Linux, the vendor's DLL or OpenSC on Windows and macOS. Reaching
  host tokens from inside Flatpak (p11-kit server forwarding) is
  unverified.
- **Recommendation:** **adopt cryptoki.** High.
