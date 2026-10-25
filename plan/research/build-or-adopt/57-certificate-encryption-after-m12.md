# Build or adopt 57: Certificate encryption (`/Adobe.PubSec`), after M12

Area: M12: Signatures · Part of [build or adopt](README.md)

- **Need:** decrypt: find the user's recipient in `/Recipients` (CMS
  EnvelopedData), recover the 20-byte seed by RSA key transport (PKCS#1
  v1.5 or OAEP) or ECDH, decrypt the envelope (AES, 3DES or RC2), derive
  the file key with SHA-1 (s4) or SHA-256 (AES-256). Encrypt for chosen
  recipients.
- **Where:** after M12; M11 slice 1's refusal.
- **Today's plan:** "Certificate encryption (`/Adobe.PubSec`) comes after
  M12 … PDFium only opens the standard (password) handler"
  (`m11.md:69-72`); "It needs the certificate stores built here, and
  sheaf has to decrypt these files itself" (`ROADMAP.md:411-415`);
  "Certificate-encrypted files get a clear refusal" (`m11.md:425`).
- **Corpus:** 12 such files in `signatures/pyhanko/pdf/`, 9 with
  adbe.pkcs7.s5 and 3 with s4 (`minimal-pubkey-aes256`, `-rc4`,
  `-rc4-envelope`, `-unknown-envelope-alg`, `pubkey-3des-test`,
  `pubkey-rc2-test`, `pubkey-ecc-test` and others). The keys that open
  them are in pyHanko's repository (MIT).
- **Candidates:** cms 0.3 (EnvelopedData, its recipient types and
  builders; `src/enveloped_data.rs:33-218`, `builder/kari.rs`);
  [des](https://crates.io/crates/des) 0.9.0 and
  [rc2](https://crates.io/crates/rc2) 0.9.0 (2026-04-10, RustCrypto),
  plus aes, cbc, sha1 and sha2; p256 and p384 with ansi-x963-kdf for
  ECDH. Key unwrap: the rsa crate (Marvin is about exactly this kind of
  PKCS#1 v1.5 decryption; local use only), aws-lc-rs'
  `Pkcs1PrivateDecryptingKey` (constant-time, a C build), or the key
  stores' own decrypt (NCryptDecrypt, `SecKeyCreateDecryptedData`,
  C_Decrypt). Prior art: pyHanko's `PubKeySecurityHandler`
  ([docs](https://docs.pyhanko.eu/en/latest/api-docs/pyhanko.pdf_utils.crypt.html)):
  the s3, s4 and s5 subfilters, RSA PKCS#1 v1.5 or OAEP key transport,
  ECDH ephemeral-static with the X9.63 KDF, AES, AES-GCM and RC4 crypt
  filters.
- **Recommendation:** **build on cms and RustCrypto,** with the key
  operation behind M12's key-store trait, which gains a decrypt. Medium.
  M11 slice 1's refusal can be tested on these 12 files now.
