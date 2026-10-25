# Build or adopt 93: B5: a signature already broken still asks

Area: Backlog items · Part of [build or adopt](README.md)

- **Need:** before the signature ask, know whether each signature's
  `/ByteRange` digest still matches what its CMS says (the messageDigest
  signed attribute, or a document timestamp's TSTInfo messageImprint).
  One already broken doesn't ask again.
- **Where:** `backlog.md:184-209` (`forms::sigs::sig_impact`,
  `crates/sheaf-doc/src/forms/sigs.rs:187`); the tests are F9's
  (`:665`).
- **Today's plan:** "probably needs the digest check pyHanko does"
  (`backlog.md:198`).
- **The spike:**
  - Dependencies: cms 0.3.0-rc.0, der 0.8 (features `ber`, `alloc`,
    `oid`), sha1 0.11, sha2 0.11, md-5 0.11 and spki 0.8.
  - Core: about 40 lines. Decode ContentInfo then SignedData with BER
    rules, without calling `finish()`, so the padding is ignored. Take
    the first SignerInfo's `digest_alg` and messageDigest, or the TSTInfo
    imprint, then hash the two ranges.
  - Results: 493 messageDigest matches, 53 timestamp imprint matches, 2
    mismatches. 28 have no signed attributes: their digest is inside the
    signature, so checking them needs entry 45's RSA public operation and
    the certificate. 12 use digests the spike didn't hash (GOST,
    SHAKE256, and one sha1WithRSA OID given as a digest algorithm, which
    pyHanko maps to SHA-1). 5 had parse problems; 18 had `/ByteRange` or
    `/Contents` shapes the spike skips.
  - B5's named files: pyHanko finds neither intact; the spike agrees.
  - Differences from pyHanko's manifest counts, file by file: most come
    from the manifest's pyHanko run failing on document timestamps
    ("Signature object type must be /Sig") or on asn1crypto parse
    errors; others are pyHanko test vectors with unusual ByteRanges,
    which the spike skips.
  - Speed: 173.6 MB hashed in 0.37 s, one thread, release build, parsing
    included.
- **Fit:** small; sheaf already reads the signature dictionaries
  (`forms/sigs.rs`).
- **Recommendation:** **build on cms 0.3.0-rc.0, der 0.8, sha1 and
  sha2,** pinned exactly. High. Compute it lazily, once per source,
  cancellable: a signature covers nearly the whole file, so N signatures
  cost N hashes of it. Count a signature as broken only on a clear
  mismatch; unknown algorithms, no signed attributes and parse failures
  keep asking. Land it with F9's `signatures/` tier.
