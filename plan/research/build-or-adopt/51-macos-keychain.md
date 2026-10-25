# Build or adopt 51: macOS keychain (missing from the plan)

Area: M12: Signatures · Part of [build or adopt](README.md)

- **Need:** identities in the login and system keychains, smart cards
  through CryptoTokenKit, signing with `SecKeyCreateSignature`, trust
  settings for verification.
- **Where:** M12 slice 4. **Not in `ROADMAP.md:397-398`,** though macOS
  is a target platform. D8.
- **Candidates:** **[security-framework](https://crates.io/crates/security-framework)**
  3.7.0 (2026-02-20; MIT OR Apache-2.0; kornelski and djc; 416M
  downloads; 306★; FFI; already in the lockfile). Identity search with
  `ItemSearchOptions` (`src/item.rs:62-66`);
  `SecKey::create_signature(Algorithm, input)` and `verify_signature`
  (`src/key.rs:228-250`).
- **Recommendation:** **adopt it, and add the keychain to the plan.**
  High.
