//! Signatures' digests checked against their bytes (B5): signed in code,
//! intact and with a byte changed, a document timestamp, BER, what can't
//! be checked, `/ByteRange`s of every shape, hostile CMS; and over the
//! corpus's signed files, against pyHanko's verdicts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use lopdf::{Object, StringFormat, dictionary};
use sha2::Digest as _;

use super::*;
use crate::effects::SigClass;
use crate::forms::corpus::Corpus;
use crate::forms::fixture::{
    CmsSpec, Form, detached, oid, row, signed_data, source, timestamped, tst_info,
};
use crate::forms::sigs::{Pending, Signature, Signatures, Verdict, sig_impact};
use crate::source::Limits;
use crate::store::{BlobKind, Store};

/// A form with a text field and a signature field, signed by `cms`.
fn signed_form(cms: fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
    let mut f = Form::new(1);
    f.text("t", 0, row(0));
    let sig = f.signature("Sig", 0, row(1));
    f.sign(sig);
    f.signed_bytes(cms)
}

/// The signatures `bytes` holds, as a source reads them.
fn signatures(bytes: Vec<u8>) -> Signatures {
    Signatures::clone(source(bytes).signatures())
}

/// The integrity of `bytes`' only signature.
fn only(bytes: Vec<u8>) -> Integrity {
    let sigs = signatures(bytes);
    assert_eq!(sigs.signed.len(), 1);
    sigs.signed[0].integrity().clone()
}

/// `bytes` with the page's text, which its signature signs, changed.
fn changed(mut bytes: Vec<u8>) -> Vec<u8> {
    let at = find(&bytes, b"(Page 1)").expect("the page's text");
    bytes[at + 6] = b'7';
    bytes
}

/// What a Rewrite does to `sigs`' first signature.
fn rewritten(sigs: &Signatures) -> Verdict {
    let fields = BTreeSet::new();
    sig_impact(sigs, Pending::Rewrite, SigClass::FILL, &fields)[0].verdict
}

/// Where `needle` first is in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A detached signature signed in code is intact, and a save asks before
/// it breaks it. A byte it signs changed, it's broken, and the save that
/// would break it doesn't ask.
#[test]
fn a_signature_is_intact_until_a_byte_it_signs_changes() {
    let bytes = signed_form(detached);
    let sigs = signatures(bytes.clone());
    assert_eq!(*sigs.signed[0].integrity(), Integrity::Intact);
    assert_eq!(rewritten(&sigs), Verdict::Invalid);
    let sigs = signatures(changed(bytes));
    assert_eq!(*sigs.signed[0].integrity(), Integrity::Broken);
    assert_eq!(rewritten(&sigs), Verdict::AlreadyBroken);
    assert!(!Verdict::AlreadyBroken.asks());
}

/// A document timestamp's messageImprint is what's checked: intact, and
/// broken by a byte it covers.
#[test]
fn a_timestamp_is_checked_by_its_imprint() {
    let bytes = signed_form(timestamped);
    assert_eq!(only(bytes.clone()), Integrity::Intact);
    assert_eq!(only(changed(bytes)), Integrity::Broken);
}

/// A SignerInfo with no signed attributes signs the ranges' digest in its
/// signature value: RSA PKCS#1 v1.5, opened with the key of the signer's
/// certificate in the CMS, intact, and broken by a byte it signs. A value
/// that doesn't open to a DigestInfo, as one changed doesn't, is Unknown.
#[test]
fn an_rsa_signature_with_no_signed_attributes_is_opened() {
    fn rsa(digest: &[u8]) -> Vec<u8> {
        signed_data(&CmsSpec {
            message_digest: None,
            rsa: Some(digest),
            ..CmsSpec::detached(oid::SHA256, digest)
        })
    }
    let bytes = signed_form(rsa);
    assert_eq!(only(bytes.clone()), Integrity::Intact);
    assert_eq!(only(changed(bytes)), Integrity::Broken);
    // The signature value is the CMS's last bytes.
    let digest = sha2::Sha256::digest(b"signed");
    let signed = |contents: Vec<u8>| Signed {
        ranges: vec![(0, 6)],
        contents,
    };
    assert_eq!(
        integrity(&signed(rsa(&digest)), b"signed"),
        Integrity::Intact
    );
    let mut cms = rsa(&digest);
    let at = cms.len() - 10;
    cms[at] ^= 1;
    let unknown = Integrity::Unknown(Unchecked::NoSignedAttributes);
    assert_eq!(integrity(&signed(cms), b"signed"), unknown);
}

/// Bytes before a signed file's header (a BOM a server added after it was
/// signed): its ranges, counted from the header as sheaf reads every
/// offset, are intact, and broken by a byte they sign changed. A file
/// signed with the BOM there, its ranges counted from the first byte as
/// pyHanko counts them, is intact too.
#[test]
fn bytes_before_the_header() {
    const BOM: &[u8] = b"\xef\xbb\xbf";
    let prefixed = [BOM, &signed_form(detached)].concat();
    assert_eq!(only(prefixed.clone()), Integrity::Intact);
    assert_eq!(only(changed(prefixed)), Integrity::Broken);
    // Signed with it: the ranges' offsets past it, its length in the
    // first range.
    let mut f = Form::new(1);
    let sig = f.signature("Sig", 0, row(0));
    f.sign(sig);
    let mut bytes = [BOM, &f.bytes()].concat();
    let at = find(&bytes, b"/ByteRange").unwrap();
    let at = at + find(&bytes[at..], b"[").unwrap() + 1;
    let end = at + find(&bytes[at..], b"]").unwrap();
    let range: Vec<usize> = (std::str::from_utf8(&bytes[at..end]).unwrap())
        .split_whitespace()
        .map(|n| n.parse().unwrap())
        .collect();
    let range = format!(
        "0 {:010} {:010} {:010}",
        range[1] + 3,
        range[2] + 3,
        range[3]
    );
    bytes.splice(at..end, range.into_bytes());
    crate::forms::fixture::sign_contents(&mut bytes, detached);
    assert_eq!(only(bytes.clone()), Integrity::Intact);
    assert_eq!(only(changed(bytes)), Integrity::Broken);
}

/// What can't be checked is Unknown, and why, and a save asks as it does
/// before breaking an intact one: zeros, no signed attributes, a digest
/// algorithm sheaf doesn't compute, two SignerInfos, a messageDigest of
/// another length, a detached signature's own content whose digest isn't
/// the ranges' (and one whose is, intact), `/Contents` that isn't a
/// string.
#[test]
fn what_can_t_be_checked_is_unknown_and_asks() {
    let unknown = |why: Unchecked| Integrity::Unknown(why);
    let mut zeros = Form::new(1);
    let sig = zeros.signature("Sig", 0, row(0));
    zeros.sign(sig);
    let cases: [(Vec<u8>, Integrity); 7] = [
        (zeros.bytes(), unknown(Unchecked::Cms)),
        (
            signed_form(|d| {
                signed_data(&CmsSpec {
                    message_digest: None,
                    ..CmsSpec::detached(oid::SHA256, d)
                })
            }),
            unknown(Unchecked::NoSignedAttributes),
        ),
        (
            signed_form(|d| signed_data(&CmsSpec::detached(oid::GOST, d))),
            unknown(Unchecked::Algorithm(oid::GOST.into())),
        ),
        (
            signed_form(|d| {
                signed_data(&CmsSpec {
                    signers: 2,
                    ..CmsSpec::detached(oid::SHA256, d)
                })
            }),
            unknown(Unchecked::Cms),
        ),
        (
            signed_form(|d| signed_data(&CmsSpec::detached(oid::SHA256, &d[..20]))),
            unknown(Unchecked::Cms),
        ),
        (
            signed_form(|d| {
                let content = sha1::Sha1::digest(d);
                let digest = sha2::Sha256::digest(content);
                signed_data(&CmsSpec {
                    econtent: Some(&content),
                    ..CmsSpec::detached(oid::SHA256, &digest)
                })
            }),
            unknown(Unchecked::Encapsulated),
        ),
        (
            signed_form(|d| {
                signed_data(&CmsSpec {
                    econtent: Some(b"content"),
                    ..CmsSpec::detached(oid::SHA256, d)
                })
            }),
            Integrity::Intact,
        ),
    ];
    for (i, (bytes, want)) in cases.into_iter().enumerate() {
        let sigs = signatures(bytes);
        assert_eq!(*sigs.signed[0].integrity(), want, "case {i}");
        assert_eq!(rewritten(&sigs), Verdict::Invalid, "case {i}");
    }
    let mut f = Form::new(1);
    let sig = f.signature("Sig", 0, row(0));
    f.sign(sig);
    let v = f.pdf.get_dictionary(sig).unwrap().get(b"V").unwrap();
    let v = v.as_reference().unwrap();
    f.dict(v).set("Contents", 7);
    assert_eq!(only(f.bytes()), unknown(Unchecked::Contents));
}

/// A signature of `ranges`, its CMS a detached one whose messageDigest is
/// the SHA-256 of `hashed`.
fn signed_over(ranges: &[(u64, u64)], hashed: &[&[u8]]) -> Signed {
    let mut d = sha2::Sha256::new();
    for part in hashed {
        d.update(part);
    }
    Signed {
        ranges: ranges.to_vec(),
        contents: detached(&d.finalize()),
    }
}

/// `/ByteRange`s of every shape, hashed as pyHanko hashes them: in order,
/// each cut at the file's end, overlapping, starting anywhere, none at
/// all; a match intact, else broken. Ranges that cover more bytes than
/// the file has are Unknown.
#[test]
fn byte_ranges_are_hashed_as_given_and_cut_at_the_end() {
    let bytes: Vec<u8> = (0..100u8).collect();
    let b = &bytes[..];
    let cases: [(Signed, Integrity); 9] = [
        // As a signer writes them, around /Contents.
        (
            signed_over(&[(0, 10), (20, 80)], &[&b[..10], &b[20..]]),
            Integrity::Intact,
        ),
        // Past the file's end: what's there is hashed, and it isn't what
        // the signer hashed, which ran on.
        (
            signed_over(&[(0, 10), (20, 200)], &[&b[..10], &b[20..], &[0; 120]]),
            Integrity::Broken,
        ),
        (
            signed_over(&[(0, 10), (20, 200)], &[&b[..10], &b[20..]]),
            Integrity::Intact,
        ),
        // Starting past the end: nothing of it is hashed.
        (
            signed_over(&[(0, 10), (500, 10)], &[&b[..10]]),
            Integrity::Intact,
        ),
        // One range, a gap that isn't a /Contents, and one that doesn't
        // start at 0.
        (signed_over(&[(0, 100)], &[b]), Integrity::Intact),
        (
            signed_over(&[(5, 10), (60, 3)], &[&b[5..15], &b[60..63]]),
            Integrity::Intact,
        ),
        // Overlapping, in order.
        (
            signed_over(&[(0, 50), (25, 50)], &[&b[..50], &b[25..75]]),
            Integrity::Intact,
        ),
        // None: nothing signed, which isn't what it signed.
        (signed_over(&[], &[b"x"]), Integrity::Broken),
        // More bytes than the file has.
        (
            signed_over(&[(0, 100), (0, 100)], &[b, b]),
            Integrity::Unknown(Unchecked::ByteRange),
        ),
    ];
    for (i, (signed, want)) in cases.into_iter().enumerate() {
        assert_eq!(integrity(&signed, b), want, "case {i}: {:?}", signed.ranges);
    }
}

/// A `/ByteRange` that isn't pairs of non-negative integers, at most
/// eight, isn't hashed; nor is `/Contents` that isn't a string, or past
/// the caps.
#[test]
fn a_byte_range_that_isn_t_pairs_is_unknown() {
    let pdf = lopdf::Document::with_version("1.7");
    let limits = Limits::DEFAULT.forms;
    let sig = |range: Vec<Object>, contents: Object| {
        let dict = dictionary! { "ByteRange" => range, "Contents" => contents };
        signed(&pdf, &dict, &limits).map(|s| s.ranges)
    };
    let hex = || Object::String(vec![0; 4], StringFormat::Hexadecimal);
    let n = |v: i64| Object::Integer(v);
    let pairs = |count: usize| (0..2 * count as i64).map(n).collect::<Vec<_>>();
    assert_eq!(
        sig(vec![n(0), n(1), n(2), n(3)], hex()),
        Ok(vec![(0, 1), (2, 3)])
    );
    assert_eq!(sig(vec![], hex()), Ok(vec![]));
    let bad = [
        vec![n(0), n(1), n(2)],
        vec![n(0), n(-1)],
        vec![n(0), Object::Real(1.0)],
        vec![n(0), Object::Reference((1, 0))],
        pairs(MAX_RANGES + 1),
    ];
    for range in bad {
        assert_eq!(
            sig(range.clone(), hex()),
            Err(Unchecked::ByteRange),
            "{range:?}"
        );
    }
    assert_eq!(
        sig(pairs(MAX_RANGES), hex()).map(|r| r.len()),
        Ok(MAX_RANGES)
    );
    assert_eq!(sig(vec![n(0), n(1)], n(7)), Err(Unchecked::Contents));
    let long = Object::String(vec![0; limits.cms + 1], StringFormat::Hexadecimal);
    assert_eq!(sig(vec![n(0), n(1)], long), Err(Unchecked::Contents));
}

/// `der` with each constructed value's length made indefinite, as the
/// BER real signers write.
fn indefinite(der: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < der.len() {
        let (constructed, length, head) = header(&der[at..]).unwrap();
        let length = length.unwrap();
        assert_ne!(der[at] & 0x1f, 0x1f, "low tag numbers only");
        let body = &der[at + head..at + head + length];
        if constructed {
            out.extend([der[at], 0x80]);
            out.extend(indefinite(body));
            out.extend([0, 0]);
        } else {
            out.extend(&der[at..at + head + length]);
        }
        at += head + length;
    }
    out
}

/// BER as real signers write it: indefinite lengths, a timestamp's TSTInfo
/// in a constructed OCTET STRING; the zeros `/Contents` pads it with
/// after.
#[test]
fn ber_is_read() {
    let digest = sha2::Sha256::digest(b"signed");
    let signed = |contents: Vec<u8>| Signed {
        ranges: vec![(0, 6)],
        contents,
    };
    let mut ber = indefinite(&detached(&digest));
    ber.extend([0; 64]);
    assert_eq!(integrity(&signed(ber), b"signed"), Integrity::Intact);
    // The TSTInfo in chunks.
    let tst = tst_info(&digest);
    let mut ber = indefinite(&timestamped(&digest));
    let at = find(&ber, &tst).unwrap();
    assert_eq!(ber[at - 2..at], [0x04, tst.len() as u8]);
    let (a, b) = tst.split_at(30);
    let mut chunked = vec![0x24, 0x80, 0x04, a.len() as u8];
    chunked.extend(a);
    chunked.extend([0x04, b.len() as u8]);
    chunked.extend(b);
    chunked.extend([0, 0]);
    ber.splice(at - 2..at + tst.len(), chunked);
    assert_eq!(integrity(&signed(ber), b"signed"), Integrity::Intact);
}

/// Hostile CMS never panics, nor exhausts the stack: indefinite lengths
/// nested past [`MAX_NESTING`], every cut of a real one and a byte of it
/// changed anywhere, each is Unknown, broken or intact.
#[test]
fn hostile_cms_is_unknown_not_a_crash() {
    let deep: Vec<u8> = [[0x30, 0x80].repeat(200_000), vec![0; 400_000]].concat();
    assert!(!shallow(&deep));
    let at_most: Vec<u8> = [[0x30, 0x80].repeat(MAX_NESTING), vec![0; 2 * MAX_NESTING]].concat();
    assert!(shallow(&at_most));
    let past: Vec<u8> = [
        [0x30, 0x80].repeat(MAX_NESTING + 1),
        vec![0; 2 * MAX_NESTING + 2],
    ]
    .concat();
    assert!(!shallow(&past));
    let digest = sha2::Sha256::digest(b"signed");
    let check = |contents: Vec<u8>| {
        let signed = Signed {
            ranges: vec![(0, 6)],
            contents,
        };
        integrity(&signed, b"signed")
    };
    assert_eq!(check(deep), Integrity::Unknown(Unchecked::Cms));
    for real in [indefinite(&timestamped(&digest)), detached(&digest)] {
        for cut in 0..real.len() {
            let _ = check(real[..cut].to_vec());
        }
        for at in 0..real.len() {
            for flip in [0x01, 0x80, 0xff] {
                let mut bytes = real.clone();
                bytes[at] ^= flip;
                let _ = check(bytes);
            }
        }
    }
}

/// Fields that share a signature dictionary share its check; past the
/// caps' signatures one is Unknown; and nothing is hashed until a save
/// would break or flag one. Two signed in one revision, the second's
/// `/Contents` written after the first was signed breaks the first, as it
/// would a real one.
#[test]
fn checks_are_shared_capped_and_lazy() {
    let mut f = Form::new(1);
    let a = f.signature("A", 0, row(0));
    let b = f.signature("B", 0, row(1));
    let c = f.signature("C", 0, row(2));
    f.sign(a);
    f.sign(c);
    let v = f.pdf.get_dictionary(a).unwrap().get(b"V").unwrap().clone();
    f.dict(b).set("V", v);
    let bytes = f.signed_bytes(detached);
    let sigs = signatures(bytes.clone());
    let [sa, sb, sc] = &sigs.signed[..] else {
        panic!("{:?}", sigs.signed);
    };
    assert!(Arc::ptr_eq(&sa.check, &sb.check) && !Arc::ptr_eq(&sa.check, &sc.check));
    let hashed = |s: &Signature| s.check.verdict.get().is_some();
    assert!(![sa, sb, sc].into_iter().any(hashed));
    // Comments on an approval signature: noted, not checked.
    let fields = BTreeSet::new();
    let impact = sig_impact(&sigs, Pending::Append, SigClass::ANNOT, &fields);
    assert!(impact.iter().all(|i| i.verdict == Verdict::Noted));
    assert!(![sa, sb, sc].into_iter().any(hashed));
    let impact = sig_impact(&sigs, Pending::Rewrite, SigClass::ANNOT, &fields);
    let verdicts: Vec<Verdict> = impact.iter().map(|i| i.verdict).collect();
    let broken = Verdict::AlreadyBroken;
    assert_eq!(verdicts, [broken, broken, Verdict::Invalid]);
    assert!([sa, sb, sc].into_iter().all(hashed));
    // One signature checked at most.
    let store = Store::with_limits(Limits {
        forms: FormLimits {
            signatures: 1,
            ..Limits::DEFAULT.forms
        },
        ..Limits::DEFAULT
    });
    let kind = BlobKind::file(Path::new("form.pdf"), None);
    let src = store.open(bytes.into(), kind).unwrap();
    let integrity: Vec<Integrity> = (src.signatures().signed.iter())
        .map(|s| s.integrity().clone())
        .collect();
    let too_many = Integrity::Unknown(Unchecked::TooMany);
    assert_eq!(integrity, [Integrity::Broken, Integrity::Broken, too_many]);
}

/// Every cell of §9.3's table with a broken signature: what would ask is
/// already broken, and asks nothing; what wouldn't is as it was. One that
/// can't be checked is judged as an intact one.
#[test]
fn a_broken_signature_asks_nothing() {
    let with = |p: Option<u8>, check: Check| Signatures {
        signed: vec![Signature {
            field: "Sig".into(),
            p,
            certifies: p.is_some(),
            locks: Vec::new(),
            unreadable: false,
            check: Arc::new(check),
        }],
        usage_rights: None,
    };
    let known = |verdict: Integrity| {
        let check = Check::unknown(Unchecked::Cms);
        check.verdict.set(verdict).unwrap();
        check
    };
    let classes = [SigClass::FILL, SigClass::ANNOT, SigClass::OTHER];
    let fields = BTreeSet::from(["x".to_string()]);
    for p in [Some(1), Some(2), Some(3), None] {
        for pending in [Pending::Rewrite, Pending::Append] {
            for class in classes {
                let verdict =
                    |check| sig_impact(&with(p, check), pending, class, &fields)[0].verdict;
                let intact = verdict(known(Integrity::Intact));
                let broken = verdict(known(Integrity::Broken));
                let unknown = verdict(Check::unknown(Unchecked::Cms));
                let want = match intact.asks() {
                    true => Verdict::AlreadyBroken,
                    false => intact,
                };
                assert_eq!(broken, want, "{p:?} {pending:?} {class:?}");
                assert_eq!(unknown, intact, "{p:?} {pending:?} {class:?}");
            }
        }
    }
}

/// Corpus paths, comma-separated, to run
/// [`the_corpus_s_signatures_agree_with_pyhanko`] on alone
/// (`SHEAF_SIGS_ONLY=signatures/safedocs/0008830.pdf,…`). Unset, it takes
/// every signed file.
const SIGS_ONLY: &str = "SHEAF_SIGS_ONLY";

/// Over the corpus's signed files (`signatures/`), sheaf's verdicts agree
/// with pyHanko's on every signature sheaf can check. The manifest counts
/// pyHanko's per file (`intact`, `signatures`), so a file is compared
/// where both count its signatures alike: where pyHanko read each without
/// an error, sheaf finds as many intact, give or take those it can't
/// check; where pyHanko failed on some (document timestamps, which its
/// run refused, among them), sheaf finds none broken beyond those pyHanko
/// didn't find intact. The Unknowns are counted, by why.
#[test]
fn the_corpus_s_signatures_agree_with_pyhanko() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let mut rows = corpus.signatures().unwrap();
    rows.retain(|r| {
        r.column("signatures_read") == "ok" && !matches!(r.column("signatures"), "" | "0")
    });
    if let Ok(only) = std::env::var(SIGS_ONLY) {
        let named: BTreeSet<&str> = only.split(',').map(str::trim).collect();
        rows.retain(|r| named.contains(r.path.as_str()));
        assert_eq!(
            rows.len(),
            named.len(),
            "{SIGS_ONLY} names a file that isn't signed: {only}"
        );
    }
    let read = crate::forms::corpus::each_entry(&rows, |row| {
        let bytes = std::fs::read(corpus.path(row)).ok()?;
        let password = Some(row.column("password")).filter(|p| !p.is_empty());
        let kind = BlobKind::file(Path::new("signed.pdf"), password);
        let src = Store::new().open(bytes.into(), kind).ok()?;
        let verdicts: Vec<Integrity> = (src.signatures().signed.iter())
            .filter(|s| !s.field.is_empty())
            .map(|s| s.integrity().clone())
            .collect();
        Some(verdicts)
    });
    let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut files, mut agreed, mut bounded, mut erred) = (0, 0, 0, 0);
    let (mut unopened, mut counted, mut disagree) = (Vec::new(), Vec::new(), Vec::new());
    let mut broken = Vec::new();
    for (row, verdicts) in rows.iter().zip(&read) {
        let Some(verdicts) = verdicts else {
            unopened.push(row.path.as_str());
            continue;
        };
        files += 1;
        for v in verdicts {
            *tally.entry(v.name()).or_default() += 1;
        }
        let count = |want: &Integrity| verdicts.iter().filter(|v| *v == want).count();
        let (intact, broke) = (count(&Integrity::Intact), count(&Integrity::Broken));
        let unknown = verdicts.len() - intact - broke;
        if broke > 0 {
            broken.push(format!("{} ({broke} of {})", row.path, verdicts.len()));
        }
        let theirs: usize = row.column("signatures").parse().unwrap();
        let found: usize = row.column("intact").parse().unwrap();
        if verdicts.len() != theirs {
            counted.push(format!(
                "{}: {} here, {theirs} by pyHanko",
                row.path,
                verdicts.len()
            ));
            continue;
        }
        // pyHanko's intact ones include sheaf's; those it didn't find
        // intact, sheaf's broken ones.
        let clean = row.column("signatures_message").is_empty();
        erred += usize::from(!clean);
        let fits = found + broke <= theirs && (!clean || intact <= found);
        if !fits {
            disagree.push(format!(
                "{}: {intact} intact, {broke} broken, {unknown} unknown; pyHanko {found} of {theirs} \
                 intact{}",
                row.path,
                if clean { "" } else { ", with an error" }
            ));
        } else if clean && unknown == 0 {
            agreed += 1;
        } else {
            bounded += 1;
        }
    }
    eprintln!(
        "{files} signed files of {}: {} signatures, by verdict {tally:?}; {agreed} agree with \
         pyHanko exactly, {bounded} within what sheaf can't check or pyHanko erred on ({erred} \
         erred), {} counted otherwise, {} not opened",
        rows.len(),
        tally.values().sum::<usize>(),
        counted.len(),
        unopened.len(),
    );
    eprintln!("  broken: {}", broken.join(", "));
    for line in counted.iter().chain(&disagree) {
        eprintln!("  {line}");
    }
    assert!(
        disagree.is_empty(),
        "{} disagree with pyHanko",
        disagree.len()
    );
    // B5's two: neither signature intact.
    for path in [
        "signatures/safedocs/0008830.pdf",
        "signatures/safedocs/0010094.pdf",
    ] {
        if let Some(i) = rows.iter().position(|r| r.path == path) {
            let verdicts = read[i].as_ref().unwrap();
            assert!(
                verdicts.iter().all(|v| *v == Integrity::Broken),
                "{path}: {verdicts:?}"
            );
        }
    }
}
