# M12: Signatures

Status: sketched · Fill & Sign (slice 1) comes right after M10 slices 6–17, ahead of the rest (2026-10-23)

Proves: Engagement letters and 8879s are signed in-app · Depends on: M10 slices 23–26 if signing creates new signature fields (open)

## Summary

- Fill & Sign: typed, drawn, or image signature and initials, saved for reuse.
- Digital signatures: sign with PKCS#12 files, the Windows certificate store,
  and PKCS#11 tokens; visible and invisible signatures; signature fields.
- Signature panel: validation status, certificate details, changes since
  signing.
- RFC 3161 timestamps; LTV (embed revocation info).
- Certify documents; lock fields on sign.

Slices: Fill & Sign → sign with PKCS#12 → verify panel → Windows store and
PKCS#11 → timestamps + LTV → certify/lock.

**Fill & Sign goes early** (2026-10-23): right after M10 slices 6–17,
ahead of the rest of M12.
- Nothing blocks it. RustCrypto's cms 0.3 round (build-or-adopt D2)
  matters only to digital signatures.
- M9's dynamic stamps (name and initials from the identity in
  `settings.toml`) and image stamps already do about half of it.
- It pairs with forms: an 8879 is filled, then signed.
- **Open:** the script font for typed signatures (build-or-adopt entry
  56), and where saved signatures are kept. sheaf never writes
  `settings.toml`, and `state.json` is disposable, so they're a third
  kind of file, as [spell check](../pending.md#spell-check)'s word list is.

Signing appends: it must never rewrite earlier revisions, and M10.5's
Append (slice 10) provides that. On a signed file, a save that has to
rewrite already asks first.

At the kickoff, revisit what backlog B5's digest check
(`forms::sigs::integrity`) built on: cms 0.3.0-rc.0, a pre-release pinned
exactly, with der 0.8; and its RSA PKCS#1 v1.5 opening of a signature
value with no signed attributes, by num-bigint, which M12's verification
may replace.

**Oracles** (Greg, 2026-10-23; [docs/oracles.md](../../docs/oracles.md#by-milestone)):
pyHanko for signing and PAdES levels B-B to B-LTA, `openssl cms -verify`
for each signature's CMS, and EU DSS, the EU's reference validator.

Follow-up: certificate encryption ❓ (`/Adobe.PubSec`; moved from M11
2026-10-14). It needs the certificate stores built here, and sheaf has
to decrypt these files itself because PDFium only opens password
encryption.

## Plan files

- No plan yet: the milestone is broken into slices when it starts.
- [Build or adopt](../research/build-or-adopt/README.md): the M12 rows, and the [changes to make](../research/build-or-adopt/changes-to-make.md) at kickoff
- Open questions: [asks](../asks.md)
