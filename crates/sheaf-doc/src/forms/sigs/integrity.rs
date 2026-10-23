//! Whether a signature still signs the file's bytes (backlog B5): the
//! digest of what its `/ByteRange` spans, against the one its CMS signed:
//! the messageDigest signed attribute; for a document timestamp
//! (`/SubFilter /ETSI.RFC3161`) the TSTInfo's messageImprint; and for a
//! SignerInfo with no signed attributes, the DigestInfo its RSA signature
//! value holds, opened with the public key of the signer's certificate in
//! the CMS, as PKCS#1 v1.5 signs. Only the digest: whether the signature
//! over the attributes holds, and who signed, are M12's. A signature whose
//! bytes no longer match is broken already, so a save doesn't ask before
//! breaking it ([`super::sig_impact`]).
//!
//! It's never broken on a guess. What can't be checked is
//! [`Integrity::Unknown`], with why, and still asks: a CMS that can't be
//! read; no signed attributes and a signature value that doesn't open to
//! a DigestInfo (not RSA PKCS#1 v1.5, as PSS, ECDSA and Ed25519 aren't, or
//! no certificate of the signer's); a digest algorithm sheaf doesn't
//! compute (GOST, SHA-3); a `/ByteRange` that isn't pairs of offsets and
//! lengths. pyHanko is the prior art (`validate_sig_integrity` and
//! `byte_range_digest`), as for the corpus's `intact` column:
//! - the digest algorithm is the SignerInfo's (a `sha256WithRSAEncryption`
//!   given as one names SHA-256), a timestamp's messageImprint's, or the
//!   DigestInfo's, and the SignerInfos are exactly one;
//! - the ranges are hashed as they're given, in order, each to the end of
//!   the file at most, overlapping or not, starting anywhere and leaving
//!   out whatever they leave out: so a range past the file's end, which
//!   the bytes it signed can't still be in, is broken unless what's there
//!   matches. Unlike pyHanko, which counts them from the file's first
//!   byte, they're counted from its header as sheaf reads every offset,
//!   and where bytes come before the header, either count that matches
//!   is intact. pyHanko judges the ranges' shape apart (its coverage), and
//!   so does `forms::check` (`StaleSignature`); a save asks about a
//!   signature that matches, whatever its ranges' shape;
//! - unlike pyHanko, ranges that together cover more bytes than the file
//!   has are Unknown, which bounds the hashing at one pass over the file a
//!   signature; and so is a detached signature holding content of its own
//!   (`eContent`) whose digest doesn't match, which pyHanko refuses:
//!   `adbe.pkcs7.sha1` signs its content, not the ranges; and so is a
//!   digest of another length than its algorithm's.
//!
//! The CMS is read as BER, which real files need (indefinite lengths), and
//! only as far as the digest needs: certificates are read only for a
//! SignerInfo with no signed attributes, one at a time, and CRLs never.
//! It's untrusted input: it's capped ([`FormLimits::cms`]), its indefinite
//! lengths may nest only so deep, as der measures one by recursing into
//! what it holds ([`MAX_NESTING`]), and an RSA key only so large
//! ([`MAX_MODULUS`]).
//!
//! The cms crate is 0.3.0-rc.0, a pre-release pinned exactly (Cargo.toml):
//! revisit at M12's kickoff, which builds signing and verifying on it, and
//! whose RSA verification may replace the opening here.

use std::fmt;
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use cms::cert::x509::Certificate;
use cms::cert::x509::ext::pkix::SubjectKeyIdentifier;
use cms::content_info::ContentInfo;
use cms::signed_data::{EncapsulatedContentInfo, SignerIdentifier, SignerInfo, SignerInfos};
use cms::spki::AlgorithmIdentifierOwned;
use der::asn1::{ObjectIdentifier, OctetString, UintRef};
use der::{Any, Decode, EncodingRules, Reader, SliceReader, Tag, Tagged};
use lopdf::{Dictionary, Object};
use num_bigint::BigUint;

use crate::attachments::{Objects, resolve};
use crate::source::FormLimits;

/// Whether a signature's signed bytes still match the digest it signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Integrity {
    /// They match: the bytes are as they were signed.
    Intact,
    /// They don't: what it signed changed after, or isn't in the file.
    Broken,
    /// It can't be checked, and why: a save treats it as intact.
    Unknown(Unchecked),
}

/// Why a signature's digest can't be checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unchecked {
    /// Its dictionary can't be read.
    Unreadable,
    /// Past the signatures a source checks ([`FormLimits::signatures`]).
    TooMany,
    /// Its `/ByteRange` isn't pairs of offsets and lengths, at most
    /// [`MAX_RANGES`] of them, covering no more bytes than the file has.
    ByteRange,
    /// Its `/Contents` isn't a string, or is longer than
    /// [`FormLimits::cms`].
    Contents,
    /// Its `/Contents` isn't CMS SignedData with one SignerInfo, as far as
    /// the digest needs (or nests its lengths past [`MAX_NESTING`]).
    Cms,
    /// It has no signed attributes, and its signature value, which holds
    /// its digest, doesn't open to one: it isn't RSA PKCS#1 v1.5, or the
    /// CMS hasn't the signer's certificate, or its key opens it to
    /// something else.
    NoSignedAttributes,
    /// A detached signature holding content of its own, whose
    /// messageDigest is that content's, not the ranges'.
    Encapsulated,
    /// A digest algorithm sheaf doesn't compute, by its OID.
    Algorithm(String),
}

impl Integrity {
    pub fn is_broken(&self) -> bool {
        *self == Integrity::Broken
    }

    /// A short name for it, as JSON and the tests tally it.
    pub fn name(&self) -> &'static str {
        match self {
            Integrity::Intact => "intact",
            Integrity::Broken => "broken",
            Integrity::Unknown(why) => why.name(),
        }
    }
}

impl Unchecked {
    pub fn name(&self) -> &'static str {
        match self {
            Unchecked::Unreadable => "unreadable",
            Unchecked::TooMany => "too many",
            Unchecked::ByteRange => "byte range",
            Unchecked::Contents => "contents",
            Unchecked::Cms => "cms",
            Unchecked::NoSignedAttributes => "no signed attributes",
            Unchecked::Encapsulated => "encapsulated",
            Unchecked::Algorithm(_) => "algorithm",
        }
    }
}

/// The most `/ByteRange` pairs hashed. A signer writes two.
pub const MAX_RANGES: usize = 8;

/// The largest RSA modulus a signature value is opened with, in bytes
/// (8192 bits), and its public exponent's, so opening one costs little.
pub const MAX_MODULUS: usize = 1024;
const MAX_EXPONENT: usize = 8;

/// How deep a CMS's indefinite lengths may nest. der measures one by
/// reading what it holds, recursing into each indefinite length inside,
/// so a deeper nesting would exhaust the stack. A real one nests a few.
pub const MAX_NESTING: usize = 32;

/// What a signature dictionary says it signed: its `/ByteRange`'s
/// (offset, length) pairs, and its `/Contents`, the CMS.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Signed {
    ranges: Vec<(u64, u64)>,
    contents: Vec<u8>,
}

/// What `sig` says it signed, within `limits`: its `/ByteRange`, pairs of
/// non-negative integers, and its `/Contents`, a string.
pub(crate) fn signed<P: Objects + ?Sized>(
    pdf: &P,
    sig: &Dictionary,
    limits: &FormLimits,
) -> Result<Signed, Unchecked> {
    let contents = match sig.get(b"Contents").ok().and_then(|c| resolve(pdf, c)) {
        Some(Object::String(s, _)) if s.len() <= limits.cms => s.clone(),
        _ => return Err(Unchecked::Contents),
    };
    let ranges = match sig.get(b"ByteRange").ok().and_then(|r| resolve(pdf, r)) {
        Some(Object::Array(a)) if a.len() % 2 == 0 && a.len() <= 2 * MAX_RANGES => a,
        _ => return Err(Unchecked::ByteRange),
    };
    let number = |o: &Object| u64::try_from(o.as_i64().ok()?).ok();
    let ranges = (ranges.chunks(2))
        .map(|pair| Some((number(&pair[0])?, number(&pair[1])?)))
        .collect::<Option<Vec<_>>>()
        .ok_or(Unchecked::ByteRange)?;
    Ok(Signed { ranges, contents })
}

/// Whether what `signed` says it signed still matches `bytes`, the file
/// its `/ByteRange` spans: its offsets counted from the file's header, as
/// sheaf reads every offset in a file (lopdf's `header_offset`), and where
/// bytes come before the header (a BOM a server added), from the first
/// byte too, as pyHanko counts them. A signer may have signed the file
/// with those bytes or without them, so either that matches is intact.
pub(crate) fn integrity(signed: &Signed, bytes: &[u8]) -> Integrity {
    let claim = match claimed(&signed.contents) {
        Ok(claim) => claim,
        Err(why) => return Integrity::Unknown(why),
    };
    let header = (bytes.windows(5)).position(|w| w == b"%PDF-").unwrap_or(0) as u64;
    let from_header: Vec<(u64, u64)> = (signed.ranges.iter())
        .map(|&(start, length)| (start.saturating_add(header), length))
        .collect();
    let verdict = compared(&claim, &from_header, bytes);
    if header == 0 || verdict == Integrity::Intact {
        return verdict;
    }
    match compared(&claim, &signed.ranges, bytes) {
        Integrity::Intact => Integrity::Intact,
        _ => verdict,
    }
}

/// Whether `claim` is the digest of what `ranges` span in `bytes`.
fn compared(claim: &Claim, ranges: &[(u64, u64)], bytes: &[u8]) -> Integrity {
    let Some(spans) = spans(ranges, bytes.len()) else {
        return Integrity::Unknown(Unchecked::ByteRange);
    };
    let parts = spans.into_iter().map(|span| &bytes[span]);
    let Some(digest) = digest(&claim.algorithm, parts) else {
        return Integrity::Unknown(Unchecked::Algorithm(claim.algorithm.to_string()));
    };
    // A digest of another length isn't one a mismatch says anything of:
    // a messageDigest in BER's constructed form reads as its chunks.
    if digest.len() != claim.digest.len() {
        return Integrity::Unknown(Unchecked::Cms);
    }
    match (digest == claim.digest, claim.encapsulated) {
        (true, _) => Integrity::Intact,
        (false, false) => Integrity::Broken,
        (false, true) => Integrity::Unknown(Unchecked::Encapsulated),
    }
}

/// `ranges` as spans of a file of `len` bytes, each cut at its end, as
/// pyHanko reads them; None when together they cover more bytes than the
/// file has, which only ranges that overlap do.
fn spans(ranges: &[(u64, u64)], len: usize) -> Option<Vec<Range<usize>>> {
    let mut total = 0usize;
    let spans: Vec<Range<usize>> = (ranges.iter())
        .map(|&(start, length)| {
            let start = usize::try_from(start).unwrap_or(usize::MAX).min(len);
            let end = start.saturating_add(usize::try_from(length).unwrap_or(usize::MAX));
            start..end.min(len)
        })
        .collect();
    for span in &spans {
        total = total.checked_add(span.len())?;
    }
    (total <= len).then_some(spans)
}

/// The digest a signature's CMS says the file has.
struct Claim {
    algorithm: ObjectIdentifier,
    digest: Vec<u8>,
    /// A detached signature that holds content of its own.
    encapsulated: bool,
}

/// id-signedData, id-ct-TSTInfo and the messageDigest attribute (RFC 5652,
/// RFC 3161).
const SIGNED_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.2");
const TST_INFO: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.1.4");
const MESSAGE_DIGEST: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.4");

/// The digest the CMS `cms` claims, read as BER from its first value: the
/// zeros a signer pads `/Contents` with after it are left alone.
fn claimed(cms: &[u8]) -> Result<Claim, Unchecked> {
    if !shallow(cms) {
        return Err(Unchecked::Cms);
    }
    let fail = |_| Unchecked::Cms;
    let mut r = SliceReader::new_with_encoding_rules(cms, EncodingRules::Ber).map_err(fail)?;
    let whole = ContentInfo::decode(&mut r).map_err(fail)?;
    if whole.content_type != SIGNED_DATA || whole.content.tag() != Tag::Sequence {
        return Err(Unchecked::Cms);
    }
    let value = whole.content.value();
    let mut r = SliceReader::new_with_encoding_rules(value, EncodingRules::Ber).map_err(fail)?;
    // SignedData's version and digestAlgorithms, then what it signs; its
    // certificates ([0]) kept for a SignerInfo with no signed attributes,
    // and its CRLs ([1]) skipped.
    let (_, _): (Any, Any) = (r.decode().map_err(fail)?, r.decode().map_err(fail)?);
    let content: EncapsulatedContentInfo = r.decode().map_err(fail)?;
    let mut certificates = None;
    while Tag::peek(&r).is_ok_and(Tag::is_context_specific) {
        let field: Any = r.decode().map_err(fail)?;
        if matches!(field.tag(), Tag::ContextSpecific { number, .. } if number.value() == 0) {
            certificates = Some(field);
        }
    }
    let infos: SignerInfos = r.decode().map_err(fail)?;
    let mut infos = infos.0.iter();
    let (Some(info), None) = (infos.next(), infos.next()) else {
        return Err(Unchecked::Cms);
    };
    if content.econtent_type == TST_INFO {
        let tst = content.econtent.as_ref().ok_or(Unchecked::Cms)?;
        return imprint(tst).ok_or(Unchecked::Cms);
    }
    let encapsulated = content.econtent.is_some();
    let Some(attrs) = info.signed_attrs.as_ref() else {
        let opened = opened(info, certificates.as_ref());
        let (algorithm, digest) = opened.ok_or(Unchecked::NoSignedAttributes)?;
        return Ok(Claim {
            algorithm,
            digest,
            encapsulated,
        });
    };
    let mut digests = attrs.iter().filter(|a| a.oid == MESSAGE_DIGEST);
    let (Some(attr), None) = (digests.next(), digests.next()) else {
        return Err(Unchecked::Cms);
    };
    let mut values = attr.values.iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return Err(Unchecked::Cms);
    };
    let digest: OctetString = (value.decode_as_encoding(EncodingRules::Ber)).map_err(fail)?;
    Ok(Claim {
        algorithm: info.digest_alg.oid,
        digest: digest.as_bytes().to_vec(),
        encapsulated,
    })
}

/// rsaEncryption, and the PKCS#1 v1.5 signature algorithms by digest
/// (RFC 8017): MD5, SHA-1, SHA-256, SHA-384, SHA-512, SHA-224.
const RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
const RSA_PKCS1: [&str; 6] = [
    "1.2.840.113549.1.1.4",
    "1.2.840.113549.1.1.5",
    "1.2.840.113549.1.1.11",
    "1.2.840.113549.1.1.12",
    "1.2.840.113549.1.1.13",
    "1.2.840.113549.1.1.14",
];

/// The digest a SignerInfo with no signed attributes signed, and its
/// algorithm: its signature value, RSA PKCS#1 v1.5, opened with the key of
/// the signer's certificate among `certificates` into a DigestInfo. None
/// when it isn't RSA PKCS#1 v1.5, the certificate isn't there, or the
/// value doesn't open to a DigestInfo, as with another key it wouldn't.
fn opened(info: &SignerInfo, certificates: Option<&Any>) -> Option<(ObjectIdentifier, Vec<u8>)> {
    let algorithm = info.signature_algorithm.oid;
    if algorithm != RSA && !RSA_PKCS1.contains(&algorithm.to_string().as_str()) {
        return None;
    }
    let certificate = signer(info, certificates?)?;
    let key = certificate.tbs_certificate().subject_public_key_info();
    if key.algorithm.oid != RSA {
        return None;
    }
    let key = key.subject_public_key.as_bytes()?;
    let mut r = SliceReader::new_with_encoding_rules(key, EncodingRules::Ber).ok()?;
    let (n, e) = (r
        .sequence(|k| Ok::<_, der::Error>((k.decode::<UintRef>()?, k.decode::<UintRef>()?))))
    .ok()?;
    let (n, e) = (n.as_bytes(), e.as_bytes());
    if n.len() > MAX_MODULUS || e.len() > MAX_EXPONENT {
        return None;
    }
    let (modulus, s) = (
        BigUint::from_bytes_be(n),
        BigUint::from_bytes_be(info.signature.as_bytes()),
    );
    if s >= modulus || n.len() < 64 {
        return None;
    }
    let m = s.modpow(&BigUint::from_bytes_be(e), &modulus).to_bytes_be();
    // EM = 0x00 0x01 PS 0x00 T, PS at least 8 bytes of 0xFF: the leading
    // zero is the one to_bytes_be drops.
    let em = [vec![0; n.len() - m.len()], m].concat();
    let rest = em.strip_prefix(&[0, 1])?;
    let ps = rest.iter().take_while(|&&b| b == 0xff).count();
    let t = rest[ps..].strip_prefix(&[0])?;
    if ps < 8 {
        return None;
    }
    let mut r = SliceReader::new(t).ok()?;
    let (algorithm, digest) = (r.sequence(|d| {
        let algorithm: AlgorithmIdentifierOwned = d.decode()?;
        let digest: OctetString = d.decode()?;
        Ok::<_, der::Error>((algorithm.oid, digest.as_bytes().to_vec()))
    }))
    .ok()?;
    r.finish().ok()?;
    Some((algorithm, digest))
}

/// The certificate among `certificates` (SignedData's `[0]`, a SET OF
/// CertificateChoices) that `info`'s signer identifier names. Each is
/// read on its own: what isn't an X.509 certificate is passed over.
fn signer(info: &SignerInfo, certificates: &Any) -> Option<Certificate> {
    let named = |c: &Certificate| {
        let tbs = c.tbs_certificate();
        match &info.sid {
            SignerIdentifier::IssuerAndSerialNumber(id) => {
                tbs.issuer() == &id.issuer && tbs.serial_number() == &id.serial_number
            }
            SignerIdentifier::SubjectKeyIdentifier(id) => (tbs
                .get_extension::<SubjectKeyIdentifier>())
            .is_ok_and(|k| k.is_some_and(|(_, k)| k == *id)),
        }
    };
    let value = certificates.value();
    let mut r = SliceReader::new_with_encoding_rules(value, EncodingRules::Ber).ok()?;
    while !r.is_finished() && r.peek_byte() != Some(0) {
        let choice: Any = r.decode().ok()?;
        if choice.tag() != Tag::Sequence {
            continue;
        }
        let Ok(certificate) = choice.decode_as_encoding::<Certificate>(EncodingRules::Ber) else {
            continue;
        };
        if named(&certificate) {
            return Some(certificate);
        }
    }
    None
}

/// A timestamp token's messageImprint: its eContent, an OCTET STRING
/// holding TSTInfo, read as `(hashAlgorithm, hashedMessage)`. An OCTET
/// STRING in BER's constructed form, its value the chunks, is read whole
/// (der keeps the chunks as its value): TSTInfo is a SEQUENCE, so a value
/// that starts as an OCTET STRING is chunks.
fn imprint(tst: &Any) -> Option<Claim> {
    if tst.tag() != Tag::OctetString {
        return None;
    }
    let value = tst.value();
    let joined;
    let info = match value.first() {
        Some(&0x04) => {
            joined = chunks(value)?;
            &joined[..]
        }
        _ => value,
    };
    let mut r = SliceReader::new_with_encoding_rules(info, EncodingRules::Ber).ok()?;
    let read = r.sequence(|t| {
        let (_version, _policy): (Any, Any) = (t.decode()?, t.decode()?);
        let imprint = t.sequence(|m| {
            let algorithm: AlgorithmIdentifierOwned = m.decode()?;
            let hashed: OctetString = m.decode()?;
            Ok::<_, der::Error>((algorithm.oid, hashed.as_bytes().to_vec()))
        })?;
        while !t.is_finished() {
            let _: Any = t.decode()?;
        }
        Ok::<_, der::Error>(imprint)
    });
    let (algorithm, digest) = read.ok()?;
    Some(Claim {
        algorithm,
        digest,
        encapsulated: false,
    })
}

/// The chunks of a constructed OCTET STRING's value joined: primitive
/// OCTET STRINGs, up to the value's end or an end-of-contents.
fn chunks(value: &[u8]) -> Option<Vec<u8>> {
    let mut r = SliceReader::new_with_encoding_rules(value, EncodingRules::Ber).ok()?;
    let mut out = Vec::new();
    while !r.is_finished() && r.peek_byte() != Some(0) {
        let chunk: OctetString = r.decode().ok()?;
        out.extend_from_slice(chunk.as_bytes());
    }
    Some(out)
}

/// Whether the first value in `ber` nests its indefinite lengths no deeper
/// than [`MAX_NESTING`], and its values fit inside what holds them: a
/// walk with a stack of its own, which der's reading needs first.
fn shallow(ber: &[u8]) -> bool {
    // The end of each constructed value the walk is inside: None for an
    // indefinite length, which an end-of-contents ends.
    let mut open: Vec<Option<usize>> = Vec::new();
    let mut indefinite = 0;
    let mut at = 0;
    loop {
        // Where what's open ends at the latest: the nearest definite end.
        let limit = open.iter().rev().find_map(|end| *end).unwrap_or(ber.len());
        match open.last() {
            None if at > 0 => return true,
            Some(&Some(end)) if at == end => {
                open.pop();
                continue;
            }
            Some(&None) if ber[..limit].get(at..at + 2) == Some(&[0, 0]) => {
                at += 2;
                indefinite -= 1;
                open.pop();
                continue;
            }
            _ => {}
        }
        let Some((constructed, length, header)) = ber.get(at..limit).and_then(header) else {
            return false;
        };
        at += header;
        match (constructed, length) {
            (true, None) => {
                indefinite += 1;
                if indefinite > MAX_NESTING {
                    return false;
                }
                open.push(None);
            }
            (_, Some(length)) if length > limit - at => return false,
            (true, Some(length)) => open.push(Some(at + length)),
            (false, Some(length)) => at += length,
            (false, None) => return false,
        }
        if open.len() > 4 * MAX_NESTING {
            return false;
        }
    }
}

/// A BER header at the start of `ber`: whether it's constructed, its
/// length (None for an indefinite one), and its own length.
fn header(ber: &[u8]) -> Option<(bool, Option<usize>, usize)> {
    let &first = ber.first()?;
    let mut at = 1;
    if first & 0x1f == 0x1f {
        // A high tag number: base-128 bytes, the last without its top bit.
        loop {
            let &b = ber.get(at)?;
            at += 1;
            if b & 0x80 == 0 {
                break;
            }
            if at > 5 {
                return None;
            }
        }
    }
    let &len = ber.get(at)?;
    at += 1;
    let length = match len {
        0x80 => None,
        n if n < 0x80 => Some(usize::from(n)),
        n @ 0x81..=0x84 => {
            let bytes = ber.get(at..at + usize::from(n - 0x80))?;
            at += bytes.len();
            Some(bytes.iter().fold(0usize, |l, &b| (l << 8) | usize::from(b)))
        }
        _ => return None,
    };
    Some((first & 0x20 != 0, length, at))
}

/// The digest of `parts` by the algorithm `oid` names; None for one sheaf
/// doesn't compute.
fn digest<'a>(oid: &ObjectIdentifier, parts: impl Iterator<Item = &'a [u8]>) -> Option<Vec<u8>> {
    fn of<'a, D: sha2::Digest>(parts: impl Iterator<Item = &'a [u8]>) -> Vec<u8> {
        let mut d = D::new();
        for part in parts {
            d.update(part);
        }
        d.finalize().to_vec()
    }
    Some(match oid.to_string().as_str() {
        "1.3.14.3.2.26" => of::<sha1::Sha1>(parts),
        "2.16.840.1.101.3.4.2.1" => of::<sha2::Sha256>(parts),
        "2.16.840.1.101.3.4.2.2" => of::<sha2::Sha384>(parts),
        "2.16.840.1.101.3.4.2.3" => of::<sha2::Sha512>(parts),
        "2.16.840.1.101.3.4.2.4" => of::<sha2::Sha224>(parts),
        "2.16.840.1.101.3.4.2.5" => of::<sha2::Sha512_224>(parts),
        "2.16.840.1.101.3.4.2.6" => of::<sha2::Sha512_256>(parts),
        "1.2.840.113549.2.5" => of::<md5::Md5>(parts),
        // A PKCS#1 v1.5 signature algorithm given as the digest algorithm,
        // which names its digest.
        "1.2.840.113549.1.1.4" => of::<md5::Md5>(parts),
        "1.2.840.113549.1.1.5" => of::<sha1::Sha1>(parts),
        "1.2.840.113549.1.1.11" => of::<sha2::Sha256>(parts),
        "1.2.840.113549.1.1.12" => of::<sha2::Sha384>(parts),
        "1.2.840.113549.1.1.13" => of::<sha2::Sha512>(parts),
        "1.2.840.113549.1.1.14" => of::<sha2::Sha224>(parts),
        _ => return None,
    })
}

/// A signature's [`Integrity`], checked once, when first asked: what was
/// read of its dictionary (or why nothing was), and the source's bytes it
/// spans. The fields that share a signature dictionary share one.
pub(crate) struct Check {
    signed: Result<Signed, Unchecked>,
    bytes: Arc<[u8]>,
    verdict: OnceLock<Integrity>,
}

impl Check {
    pub(crate) fn new(signed: Result<Signed, Unchecked>, bytes: Arc<[u8]>) -> Check {
        Check {
            signed,
            bytes,
            verdict: OnceLock::new(),
        }
    }

    /// One that can't be checked, for `why`.
    pub(crate) fn unknown(why: Unchecked) -> Check {
        Check::new(Err(why), Arc::from([]))
    }

    pub(crate) fn get(&self) -> &Integrity {
        self.verdict.get_or_init(|| match &self.signed {
            Ok(signed) => integrity(signed, &self.bytes),
            Err(why) => Integrity::Unknown(why.clone()),
        })
    }
}

impl fmt::Debug for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("Check");
        match &self.signed {
            Ok(s) => d
                .field("ranges", &s.ranges)
                .field("contents", &s.contents.len()),
            Err(why) => d.field("unchecked", why),
        };
        d.field("verdict", &self.verdict.get()).finish()
    }
}

#[cfg(test)]
pub(crate) mod tests;
