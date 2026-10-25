# Build or adopt 29: The Standard security handler: read R2–R6, keep on save, write AES-256

Area: M11: Security and redaction · Part of [build or adopt](README.md)

- **Need:** decrypt and encrypt every Standard-handler revision (RC4
  40–128; AES-128, V4; AES-256, R5, Adobe's deprecated extension, and
  R6); authenticate user and owner passwords; read and keep `/P`; write
  new R6 encryption from Security….
- **Where:** M11 slices 1–2 and slice 1's leftovers; F13.
- **Today's plan:** lopdf's handler, with sheaf's serializer encrypting
  each object. "Every save keeps a file's encryption exactly"
  (`m11.md:211`); "Kept on every save since M10.5 … `serialize/crypt.rs`
  writes each object with the file's key" (`:216-218`); "New encryption
  is AES-256 (R6) only" (`:219`), "written with `/Length 256`"
  (`:221-225`, `:442`); "Work around lopdf #584 for reading" (`:424`).
  F13: "encryption, which sheaf would keep on the RustCrypto crates and
  not hand-roll" (`backlog.md:702-703`).
- **What sheaf and lopdf use today** (vendored lopdf at `39492cb`):
  - lopdf: aes 0.9.3, cbc 0.2.1, ecb 0.2.1 (R6's `/Perms`), md-5 0.11.0,
    sha2 0.11.0 (R6's Algorithm 2.B); its own RC4
    (`vendor/lopdf/src/encryption/rc4.rs`, 66 lines); stringprep 0.1.5
    (SASLprep for R5/R6 passwords); rand 0.10 and getrandom 0.4.
  - sheaf-doc: hmac 0.13 and sha2 0.11 for deterministic AES IVs
    (`serialize/crypt.rs:15-45`); zeroize 1.9.1 for passwords
    (`store.rs:42`).
  - The corpus covers every revision. `parsing/` alone has rc4-40 R2/R3,
    rc4-64/80/128, aes-128 R4, aes-256 R5 (36 files) and R6 (44).
- **Candidates:**

  | Crate | Latest | License | Maintenance | Notes |
  |---|---|---|---|---|
  | [aes](https://crates.io/crates/aes) | 0.9.3, 2026-08-28 | MIT OR Apache-2.0 | RustCrypto (newpavlov); 370M downloads | Pure Rust, no_std; AES-NI by runtime detection |
  | [cbc](https://crates.io/crates/cbc) / [ecb](https://crates.io/crates/ecb) | 0.2.1, 2026-05-20 / 0.2.1, 2026-08-21 | MIT OR Apache-2.0 | RustCrypto | Pure Rust |
  | [md-5](https://crates.io/crates/md-5) | 0.11.0, 2026-03-27 | MIT OR Apache-2.0 | RustCrypto; 417M downloads | Pure Rust |
  | [sha2](https://crates.io/crates/sha2) | 0.11.1, 2026-10-12 | MIT OR Apache-2.0 | RustCrypto (tarcieri, newpavlov); 1.1B downloads | Pure Rust plus intrinsics |
  | [rc4](https://crates.io/crates/rc4) | 0.2.0, 2026-03-30 | MIT OR Apache-2.0 | RustCrypto; 1.6M downloads | Pure Rust; would replace lopdf's own RC4 |
  | [stringprep](https://crates.io/crates/stringprep) | 0.1.5, 2024-05-25 | MIT/Apache-2.0 | sfackler alone; 245M downloads (postgres' tree) | Pure Rust |

- **Fit:** complete for the Standard handler; the gaps are in lopdf's
  logic, not the primitives.
  - **#584 (the owner password giving the wrong key) is fixed in the
    vendored code.** The fix is upstream `0e781ce` ("derive the file
    encryption key from the recovered user password", PR #573,
    2026-09-12), an ancestor of the vendored `39492cb`, and it ships in
    lopdf 0.46.0. `EncryptionState::decode` calls
    `resolve_password_for_key_derivation`
    (`vendor/lopdf/src/encryption.rs:623-624`, `algorithms.rs:985-1001`).
    [Issue #584](https://github.com/J-F-Liu/lopdf/issues/584) still shows
    open.
  - **#603** is fixed in 0.46.0 (`2f497f8`, 2026-10-20), though its
    [issue](https://github.com/J-F-Liu/lopdf/issues/603) also shows open.
  - Hygiene: lopdf's `EncryptionState` keeps the file key in a plain
    `Vec`, and sheaf's `Crypt` keeps an HMAC keyed with it; neither is
    zeroized (F19).
- **Recommendation:** **build on RustCrypto**, as today. High. When F13
  brings the handler in-house, write Algorithms 2, 2.A, 2.B, 6 and 7 on
  these crates, with `Zeroizing` keys. Swapping lopdf's RC4 for the rc4
  crate, or keeping its 66 lines, are both fine. AES-GCM (ISO/TS 32003)
  isn't planned; RustCrypto's aes-gcm would cover it.
