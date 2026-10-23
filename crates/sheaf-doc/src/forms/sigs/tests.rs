//! Signatures read from forms built in code, and from the corpus; and
//! every cell of §9.3's table, FieldMDP's actions and PDF 2.0's `/Lock /P`.

use std::path::Path;

use lopdf::Object;

use super::*;
use crate::forms::corpus::Corpus;
use crate::forms::fixture::{Form, row, source};
use crate::source::Limits;
use crate::store::{BlobKind, Store};

/// One signature field, `Sig`, on a page with a text field, `a.b`.
fn signable() -> (Form, lopdf::ObjectId) {
    let mut f = Form::new(1);
    let b = f.text("b", 0, row(1));
    f.group("a", &[b]);
    let sig = f.signature("Sig", 0, row(0));
    (f, sig)
}

fn read_back(f: &mut Form) -> Signatures {
    Signatures::clone(f.source().signatures())
}

fn approval(field: &str) -> Signature {
    Signature {
        field: field.into(),
        p: None,
        certifies: false,
        locks: Vec::new(),
        unreadable: false,
        check: Arc::new(Check::unknown(Unchecked::Cms)),
    }
}

fn certified(p: u8) -> Signature {
    Signature {
        p: Some(p),
        certifies: true,
        ..approval("Sig")
    }
}

fn names(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}

/// The verdict on `sig` of a save of `pending` and `class`, touching
/// `fields`.
fn verdict(sig: &Signature, pending: Pending, class: SigClass, fields: &[&str]) -> Verdict {
    let sigs = Signatures {
        signed: vec![sig.clone()],
        usage_rights: None,
    };
    let impact = sig_impact(&sigs, pending, class, &names(fields));
    assert_eq!(impact.len(), 1);
    assert_eq!(impact[0].signature, sig.field);
    impact[0].verdict
}

#[test]
fn a_certification_s_p_and_its_default() {
    for p in 1..=3 {
        let (mut f, sig) = signable();
        f.certify(sig, p);
        assert_eq!(read_back(&mut f).signed, [certified(p as u8)], "P={p}");
    }
    // No /P is 2; one past 1–3 is 1.
    for (p, reads) in [(None, 2), (Some(Object::Integer(7)), 1)] {
        let (mut f, sig) = signable();
        f.certify(sig, 3);
        let v = f.pdf.get_dictionary(sig).unwrap().get(b"V").unwrap();
        let v = v.as_reference().unwrap();
        let reference = f.dict(v).get_mut(b"Reference").unwrap();
        let reference = reference.as_array_mut().unwrap()[0].as_dict_mut().unwrap();
        let params = reference.get_mut(b"TransformParams").unwrap();
        let params = params.as_dict_mut().unwrap();
        match p {
            Some(p) => params.set("P", p),
            None => _ = params.remove(b"P"),
        }
        assert_eq!(read_back(&mut f).signed, [certified(reads)]);
    }
}

#[test]
fn an_approval_signature_and_usage_rights() {
    let (mut f, sig) = signable();
    f.sign(sig);
    f.usage_rights_for(&["FillIn", "Import"], &["Create", "Delete", "Copy"]);
    let sigs = read_back(&mut f);
    assert_eq!(sigs.signed, [approval("Sig")]);
    let rights = sigs.usage_rights.unwrap();
    assert_eq!(
        rights,
        UsageRights {
            fill_in: true,
            annots: AnnotOps::CREATE | AnnotOps::DELETE,
        }
    );
    // Within them: filling in, and comments added or deleted.
    assert!(rights.allow(SigClass::FILL, AnnotOps::empty()));
    assert!(rights.allow(SigClass::FILL | SigClass::ANNOT, AnnotOps::CREATE));
    assert!(rights.allow(SigClass::ANNOT, AnnotOps::CREATE | AnnotOps::DELETE));
    // Not within: a comment changed, or anything else.
    assert!(!rights.allow(SigClass::ANNOT, AnnotOps::MODIFY));
    assert!(!rights.allow(SigClass::FILL | SigClass::OTHER, AnnotOps::empty()));
    // Rights to fill in only.
    let mut f = Form::new(1);
    f.usage_rights();
    let rights = read_back(&mut f).usage_rights.unwrap();
    assert!(rights.allow(SigClass::FILL, AnnotOps::empty()));
    assert!(!rights.allow(SigClass::ANNOT, AnnotOps::CREATE));
    // A certified file with rights: both.
    let (mut f, sig) = signable();
    f.certify(sig, 2);
    f.usage_rights_for(&[], &["Create"]);
    let sigs = read_back(&mut f);
    assert_eq!(sigs.signed, [certified(2)]);
    assert!(!sigs.usage_rights.unwrap().fill_in);
    assert_eq!(read_back(&mut Form::new(1)), Signatures::default());
}

#[test]
fn field_mdp_locks_and_lock_p() {
    let (mut f, sig) = signable();
    f.sign_locking(sig, "Include", &["a", "x.y"], None);
    let include = Lock::Include(vec!["a".into(), "x.y".into()]);
    let locked = Signature {
        locks: vec![include.clone(), include],
        ..approval("Sig")
    };
    assert_eq!(read_back(&mut f).signed, [locked]);
    // PDF 2.0's /P on the lock: an approval signature that permits 2.
    let (mut f, sig) = signable();
    f.sign_locking(sig, "All", &[], Some(2));
    let sigs = read_back(&mut f);
    assert_eq!(sigs.signed[0].p, Some(2));
    assert_eq!(sigs.signed[0].locks, [Lock::All, Lock::All]);
    let (mut f, sig) = signable();
    f.sign_locking(sig, "Exclude", &["a"], Some(3));
    let sigs = read_back(&mut f);
    assert_eq!(sigs.signed[0].p, Some(3));
    assert_eq!(sigs.signed[0].locks[0], Lock::Exclude(vec!["a".into()]));
    // An action it doesn't know, and names past the caps, lock every
    // field.
    let (mut f, sig) = signable();
    f.sign_locking(sig, "Some", &["a"], None);
    assert_eq!(read_back(&mut f).signed[0].locks, [Lock::All, Lock::All]);
    // The form's names are 3 bytes at most.
    let (mut f, sig) = signable();
    f.sign_locking(sig, "Include", &["a.b.c"], None);
    let store = Store::with_limits(Limits {
        forms: FormLimits {
            name: 4,
            ..Limits::DEFAULT.forms
        },
        ..Limits::DEFAULT
    });
    let kind = BlobKind::file(Path::new("form.pdf"), None);
    let src = store.open(f.bytes().into(), kind).unwrap();
    assert_eq!(src.form().facts.damaged, None);
    assert_eq!(src.signatures().signed[0].locks, [Lock::All, Lock::All]);
}

/// A signature whose dictionary can't be read is an approval signature of
/// unknown MDP, judged as P = 1: every save asks.
#[test]
fn an_unreadable_signature_asks() {
    let (mut f, sig) = signable();
    f.dict(sig).set("V", Object::Reference((9999, 0)));
    f.dict(f.acroform).set("SigFlags", 3);
    let sigs = read_back(&mut f);
    let unreadable = Signature {
        p: Some(1),
        unreadable: true,
        ..approval("Sig")
    };
    assert_eq!(sigs.signed, [unreadable]);
    for pending in [Pending::Rewrite, Pending::Append] {
        let impact = sig_impact(&sigs, pending, SigClass::FILL, &BTreeSet::new());
        assert!(impact[0].verdict.asks(), "{pending:?}");
    }
    // A value that's no dictionary, as a form's root may pass down to
    // every field, signs nothing.
    let (mut f, sig) = signable();
    f.dict(sig).set("V", Object::string_literal(""));
    assert_eq!(read_back(&mut f).signed, []);
    // A damaged form's: one stands for them.
    let (mut f, sig) = signable();
    f.sign(sig);
    let mut chain = sig;
    for i in 0..40 {
        chain = f.group(&format!("n{i}"), &[chain]);
    }
    f.set_fields(&[chain]);
    let sigs = read_back(&mut f);
    assert!(sigs.signed[0].unreadable && sigs.signed.len() == 1);
    // A certification no field holds is read from /Perms.
    let (mut f, sig) = signable();
    f.certify(sig, 3);
    f.dict(sig).remove(b"V");
    assert_eq!(read_back(&mut f).signed, [certified(3).with_field("")]);
}

impl Signature {
    fn with_field(self, field: &str) -> Signature {
        Signature {
            field: field.into(),
            ..self
        }
    }
}

/// 1263925's signature dictionary doesn't decrypt.
#[test]
fn the_corpus_s_undecryptable_signature_asks() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let path = corpus.root.join("forms/safedocs/1263925.pdf");
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("skipped: the corpus has no {}", path.display());
        return;
    };
    let sigs = read_back_bytes(bytes);
    assert_eq!(sigs.signed.len(), 1);
    // Its field's, read where the form is whole.
    assert!(sigs.signed[0].unreadable && !sigs.signed[0].field.is_empty());
    let impact = sig_impact(&sigs, Pending::Append, SigClass::FILL, &BTreeSet::new());
    assert_eq!(impact[0].verdict, Verdict::Invalid);
}

fn read_back_bytes(bytes: Vec<u8>) -> Signatures {
    Signatures::clone(source(bytes).signatures())
}

/// §9.3's table, cell by cell.
#[test]
fn every_cell_of_the_table() {
    use Pending::{Append, Rewrite};
    use Verdict::{Allowed, Flagged, Invalid, Noted};
    let (fill, annot, other) = (SigClass::FILL, SigClass::ANNOT, SigClass::OTHER);
    let columns = [certified(1), certified(2), certified(3), approval("Sig")];
    let rows: [(Pending, [SigClass; 4], [Verdict; 4]); 3] = [
        // A Rewrite breaks every one.
        (Rewrite, [fill; 4], [Invalid; 4]),
        // An Append within what each allows.
        (
            Append,
            [fill, fill, fill | annot, fill | annot],
            [Invalid, Allowed, Allowed, Noted],
        ),
        // An Append outside it.
        (
            Append,
            [fill, annot, other, other],
            [Invalid, Invalid, Invalid, Flagged],
        ),
    ];
    for (pending, classes, verdicts) in rows {
        for ((sig, class), want) in columns.iter().zip(classes).zip(verdicts) {
            let got = verdict(sig, pending, class, &[]);
            assert_eq!(got, want, "{pending:?} {class:?} P={:?}", sig.p);
        }
    }
    assert!(!Allowed.asks() && !Noted.asks() && Flagged.asks() && Invalid.asks());
}

/// FieldMDP's actions against the fields a save touches: a field it locks
/// makes a fill outside what the signature allows.
#[test]
fn field_mdp_against_the_fields_touched() {
    let locking = |sig: Signature, lock: Lock| Signature {
        locks: vec![lock],
        ..sig
    };
    let fill = SigClass::FILL;
    let all = locking(certified(2), Lock::All);
    assert_eq!(verdict(&all, Pending::Append, fill, &[]), Verdict::Allowed);
    assert_eq!(
        verdict(&all, Pending::Append, fill, &["x"]),
        Verdict::Invalid
    );
    let include = locking(certified(2), Lock::Include(vec!["a".into()]));
    for (touched, want) in [
        (&["a"][..], Verdict::Invalid),
        (&["a.b"], Verdict::Invalid),
        // A node over one it names changes what's under it.
        (&[""], Verdict::Allowed),
        (&["ab"], Verdict::Allowed),
        (&["b", "c.a"], Verdict::Allowed),
    ] {
        let got = verdict(&include, Pending::Append, fill, touched);
        assert_eq!(got, want, "include a, {touched:?}");
    }
    let exclude = locking(approval("Sig"), Lock::Exclude(vec!["a".into()]));
    for (touched, want) in [
        (&["a"][..], Verdict::Noted),
        (&["a.b"], Verdict::Noted),
        (&["b"], Verdict::Flagged),
        (&["a", "ab"], Verdict::Flagged),
    ] {
        let got = verdict(&exclude, Pending::Append, fill, touched);
        assert_eq!(got, want, "exclude a, {touched:?}");
    }
    let parent = Lock::Include(vec!["a.b".into()]);
    assert!(parent.locks("a") && parent.locks("a.b.c") && !parent.locks("a.c"));
}

/// PDF 2.0's `/Lock /P` makes an approval signature permit what it says.
#[test]
fn lock_p_sets_the_column() {
    let lock_p = |p| Signature {
        p: Some(p),
        locks: vec![Lock::Include(Vec::new())],
        ..approval("Sig")
    };
    let annot = SigClass::ANNOT;
    assert_eq!(
        verdict(&lock_p(1), Pending::Append, SigClass::FILL, &[]),
        Verdict::Invalid
    );
    assert_eq!(
        verdict(&lock_p(2), Pending::Append, annot, &[]),
        Verdict::Invalid
    );
    assert_eq!(
        verdict(&lock_p(3), Pending::Append, annot, &["a"]),
        Verdict::Allowed
    );
    assert_eq!(
        verdict(&approval("Sig"), Pending::Append, annot, &[]),
        Verdict::Noted
    );
}

/// Over the corpus, the signatures read agree with what the form reader
/// counts, a certification as one, but for 1263925's, which can't be read
/// and so is one more; and usage rights are where it says.
#[test]
fn the_corpus_s_signatures() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let entries = corpus.forms().unwrap();
    let read = crate::forms::corpus::each_entry(&entries, |entry| {
        let bytes = std::fs::read(corpus.path(entry)).ok()?;
        let kind = BlobKind::file(Path::new("form.pdf"), None);
        let src = Store::new().open(bytes.into(), kind).ok()?;
        let facts = &src.form().facts;
        let counted = facts.signed.max(usize::from(facts.certified));
        let sigs = Signatures::clone(src.signatures());
        Some((entry.path.clone(), sigs, counted, facts.usage_rights))
    });
    let (mut files, mut signatures, mut certified, mut rights) = (0, 0, 0, 0);
    let mut unreadable = Vec::new();
    for (path, sigs, counted, usage_rights) in read.into_iter().flatten() {
        files += 1;
        signatures += sigs.signed.len();
        certified += sigs.signed.iter().filter(|s| s.certifies).count();
        rights += usize::from(usage_rights);
        let cant = sigs.signed.iter().filter(|s| s.unreadable).count();
        if cant > 0 {
            unreadable.push(path.clone());
        }
        assert_eq!(sigs.signed.len(), counted + cant, "{path}");
        assert_eq!(sigs.usage_rights.is_some(), usage_rights, "{path}");
    }
    eprintln!(
        "{files} files: {signatures} signatures, {certified} certifying; usage rights in {rights}"
    );
    assert_eq!(unreadable, ["forms/safedocs/1263925.pdf"]);
}
