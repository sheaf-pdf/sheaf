# Build or adopt 43: Passwords in memory, and remembering them

Area: M11: Security and redaction · Part of [build or adopt](README.md)

- **Need:** passwords, and later keys and PINs, wiped when dropped and
  kept out of logs and `Debug`. Remembering a document password in the
  OS keychain is something **no plan asks for** (ROADMAP.md, m10.md,
  m11.md, agent.md, the backlog and the prior-art notes checked).
- **Where:** M11 slices 1–2; M12 (PKCS#12 passwords, token PINs).
- **Today:** `Zeroizing<String>` for a file's password
  (`crates/sheaf-doc/src/store.rs:42`). Copies that can't be wiped:
  GPUI's text input, PDFium's C string, lopdf's file key (F19). GPUI
  0.3.9 already has `write_credentials`, `read_credentials` and
  `delete_credentials` (`gpui-pre-0.3.9/src/app.rs:1789-1804`): oo7's
  Secret Service on Linux, the Keychain on macOS, Credential Manager on
  Windows.
- **Candidates:** [zeroize](https://crates.io/crates/zeroize) 1.9.1
  (2026-10-06; Apache-2.0 OR MIT; RustCrypto; 801M downloads), in use;
  [secrecy](https://crates.io/crates/secrecy) 0.10.3 (2024-10-09;
  Apache-2.0 OR MIT; iqlusion; 186M downloads), which **cryptoki already
  depends on** for PINs (`AuthPin`);
  [keyring](https://crates.io/crates/keyring) 4.2.0 (2026-08-29; MIT OR
  Apache-2.0; 34M downloads), now keyring-core plus per-platform stores.
- **Recommendation:** **keep zeroize; adopt secrecy in M12** for PINs and
  PKCS#12 passwords (it comes with cryptoki). High. Document that wiping
  is best-effort. If remembering ever comes up: GPUI's credentials API
  in the app, keyring only for the CLI. High.
