# Build or adopt 54: LTV (DSS and VRI) and PAdES levels

Area: M12: Signatures · Part of [build or adopt](README.md)

- **Need:** write a `/DSS` with `/Certs`, `/OCSPs` and `/CRLs`,
  optionally `/VRI` keyed by the uppercase hex SHA-1 of the signature's
  `/Contents`, in an incremental update after signing. PAdES levels: B-B
  (signingCertificateV2, no signingTime), B-T (a signature timestamp),
  B-LT (DSS), B-LTA (a document timestamp after the DSS). Read the DSS
  and Adobe's archive attribute for validation. The verify panel's
  "changes since signing", by comparing revisions.
- **Where:** M12 slices 3, 5 and 6.
- **Today's plan:** "Signing appends: it must never rewrite earlier
  revisions, and M10.5's Append (slice 10) provides that"
  (`ROADMAP.md:407-409`).
- **Candidates (none fits; all checked):**
  - [underskrift](https://crates.io/crates/underskrift) 0.1.4
    (2026-06-01; BSD-2-Clause; Kushal Das; 3.8k downloads; 13★): signs
    and verifies B-B through B-LTA, with remote (prepared hash) signing
    and policy hooks. On lopdf 0.39 and the RustCrypto 0.2 line, bytes in
    and bytes out, 24k lines. Its `core/revision.rs` sorts later
    revisions into safe and unsafe changes: prior art for the panel.
  - [pades](https://crates.io/crates/pades) 0.1.4: **first published
    2026-10-23**, 8 downloads, Apache-2.0; its own SHA-2, RSA, DER and CMS
    (3k lines). Don't use it.
  - [pdf_signing](https://crates.io/crates/pdf_signing) 0.3.0
    (2024-11-08; MIT OR Apache-2.0): lopdf 0.34,
    cryptographic-message-syntax, PNG-only appearance, "work in
    progress".
  - [pdf_signer](https://crates.io/crates/pdf_signer) 0.3.2:
    **GPL-3.0-or-later, out**.
  - [pdf_oxide](https://crates.io/crates/pdf_oxide) 0.3.78 (its
    `signatures/` has PAdES and a TSA client) and
    [oxidize-pdf](https://crates.io/crates/oxidize-pdf) 5.5.1: whole PDF
    stacks with their own parsers, so they can't sit on sheaf's Append
    and serializer.
  - Prior art: [pyHanko](https://pypi.org/project/pyHanko/) 0.38.0
    (2026-10-17; MIT; 791★): signing and validation separate;
    `DocumentSecurityStore.register_vri` and PAdES-LT and LTA profiles
    ([dss](https://pyhanko.readthedocs.io/en/0.15.1/api-docs/pyhanko.sign.validation.dss.html),
    [validation](https://pyhanko.readthedocs.io/en/latest/api-docs/pyhanko.sign.validation.html));
    `ades_lta_validation`, after ETSI EN 319 102-1 §5.6.3. EU DSS (Java,
    LGPL): reference only.
- **Recommendation:** **build in-house**, on M10.5's Append and the
  formats crates. High that no crate fits. pyHanko, already the corpus's
  oracle, checks interop; underskrift can be a second oracle in tests.
