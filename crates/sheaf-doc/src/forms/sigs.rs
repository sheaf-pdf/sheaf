//! A file's signatures as a save sees them (M10.5 §9.3): what each allows
//! a later revision to change (a certification's DocMDP `/P`, the fields
//! FieldMDP and a field's `/Lock` lock, PDF 2.0's `/Lock /P`), and Reader's
//! usage rights (`/Perms /UR3`), read once from the source
//! ([`Source::signatures`]). [`sig_impact`] says what a save would do to
//! each signature, by §9.3's table, which stays conservative until it's
//! checked against Acrobat.
//!
//! Signature dictionaries come from untrusted files, so what's read of
//! them is capped by the source's [`FormLimits`]. A reference or lock that
//! can't be read, or is past the caps, locks every field; a signature
//! whose dictionary can't be read (as a copy read the corpus's 1263925)
//! counts as an approval signature of unknown MDP, judged as P = 1, so a
//! save always asks.
//!
//! [`Source::signatures`]: crate::Source::signatures
//! [`FormLimits`]: crate::source::FormLimits

use std::collections::BTreeSet;

use lopdf::{Dictionary, LazyPdf, Object};

use super::{Field, FieldKind, FileForm};
use crate::attachments::{Objects, resolve};
use crate::effects::{AnnotOps, SigClass};
use crate::file::pdf_text;
use crate::source::{FormLimits, Source};

/// What a file's signatures allow after them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Signatures {
    /// Its signatures: the signed fields', in tree order, then a
    /// certification no signed field holds.
    pub signed: Vec<Signature>,
    /// Reader's usage rights (`/Perms /UR3`, or the older `/UR`).
    pub usage_rights: Option<UsageRights>,
}

/// One signature, and what it allows after it.
#[derive(Debug, Clone, PartialEq)]
pub struct Signature {
    /// Its field's full name: empty for a certification no signed field
    /// holds.
    pub field: String,
    /// What it permits after it, §9.3's columns: a certification's DocMDP
    /// `/P` (2 when it says none) or its field's `/Lock /P`, the stricter;
    /// None for an approval signature that sets neither. 1 for one that
    /// can't be read.
    pub p: Option<u8>,
    /// It certifies the file (`/Perms /DocMDP`).
    pub certifies: bool,
    /// The fields it locks: its FieldMDP references' and its field's
    /// `/Lock`'s.
    pub locks: Vec<Lock>,
    /// Its dictionary can't be read.
    pub unreadable: bool,
}

/// The fields a signature locks (FieldMDP's `/Action` with its `/Fields`),
/// by full name: a name locks the field it names and the nodes under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lock {
    All,
    Include(Vec<String>),
    Exclude(Vec<String>),
}

/// What Reader's usage rights allow after them: a later revision within
/// them keeps them (proposal 34).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageRights {
    /// `/Form` has `/FillIn`.
    pub fill_in: bool,
    /// What `/Annots` grants.
    pub annots: AnnotOps,
}

/// What a save is, for a signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pending {
    Rewrite,
    Append,
}

/// What a save does to one signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigImpact {
    /// Its field's full name ([`Signature::field`]).
    pub signature: String,
    pub verdict: Verdict,
}

/// A signature after a save, by §9.3's table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// It stays valid: what changed is within what it allows.
    Allowed,
    /// It stays valid, and the reader is told what changed after it: an
    /// approval signature with no MDP.
    Noted,
    /// Its bytes stay valid, but viewers flag what changed after it: the
    /// save asks.
    Flagged,
    /// The save breaks it: the save asks.
    Invalid,
}

impl Signatures {
    /// Whether the file has none: no signature, nor usage rights.
    pub fn is_empty(&self) -> bool {
        self.signed.is_empty() && self.usage_rights.is_none()
    }
}

impl Verdict {
    /// Whether the save asks before it does this.
    pub fn asks(self) -> bool {
        matches!(self, Verdict::Flagged | Verdict::Invalid)
    }
}

impl Signature {
    /// A signature whose dictionary can't be read.
    fn unreadable(field: String, certifies: bool) -> Signature {
        Signature {
            field,
            p: Some(1),
            certifies,
            locks: Vec::new(),
            unreadable: true,
        }
    }

    /// The classes of change an Append may make and leave it valid: by its
    /// `/P`, and for an approval signature with none, filling in and
    /// comments, what Acrobat permits after one.
    pub fn allows(&self) -> SigClass {
        match self.p {
            Some(2) => SigClass::FILL,
            Some(3) | None => SigClass::FILL | SigClass::ANNOT,
            Some(_) => SigClass::empty(),
        }
    }

    /// Whether it locks a field of those `fields` names.
    pub fn locks_any(&self, fields: &BTreeSet<String>) -> bool {
        (self.locks.iter()).any(|lock| fields.iter().any(|name| lock.locks(name)))
    }
}

impl Lock {
    /// Whether it locks the node named `name`. A name it includes locks the
    /// nodes over and under it too, and one it excludes frees only itself
    /// and the nodes under it.
    pub fn locks(&self, name: &str) -> bool {
        match self {
            Lock::All => true,
            Lock::Include(names) => (names.iter()).any(|n| within(name, n) || within(n, name)),
            Lock::Exclude(names) => !names.iter().any(|n| within(name, n)),
        }
    }
}

/// Whether `name` is `of`, or a node under it.
fn within(name: &str, of: &str) -> bool {
    (name.strip_prefix(of)).is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

impl UsageRights {
    /// Whether an Append of `class`, doing `ops` to annotations, keeps
    /// them: Reader checks later revisions against them, and nothing but
    /// filling in and comments is within them.
    pub fn allow(&self, class: SigClass, ops: AnnotOps) -> bool {
        !class.contains(SigClass::OTHER)
            && (!class.contains(SigClass::FILL) || self.fill_in)
            && (!class.contains(SigClass::ANNOT) || self.annots.contains(ops))
    }
}

/// What a save of `pending`, its changes of `class` touching the fields
/// `fields` names, does to each of `sigs`' signatures (§9.3). A Rewrite
/// breaks every one. An Append within what a signature allows, locking
/// none of the fields, leaves it valid; outside that it breaks one that
/// sets a `/P`, and leaves an approval signature's bytes valid, which
/// viewers flag. P = 1 allows no change, so only an Append of none (Save
/// As of a file nothing changed, onto a copy of it) is within it.
pub fn sig_impact(
    sigs: &Signatures,
    pending: Pending,
    class: SigClass,
    fields: &BTreeSet<String>,
) -> Vec<SigImpact> {
    let verdict = |s: &Signature| {
        let within = s.allows().contains(class) && !s.locks_any(fields);
        match (pending, s.p, within) {
            (Pending::Rewrite, _, _) => Verdict::Invalid,
            (Pending::Append, Some(_), true) => Verdict::Allowed,
            (Pending::Append, Some(_), false) => Verdict::Invalid,
            (Pending::Append, None, true) => Verdict::Noted,
            (Pending::Append, None, false) => Verdict::Flagged,
        }
    };
    (sigs.signed.iter())
        .map(|s| SigImpact {
            signature: s.field.clone(),
            verdict: verdict(s),
        })
        .collect()
}

/// `/AcroForm /SigFlags`' SignaturesExist.
const SIGNATURES_EXIST: i64 = 1;

/// Read the signatures in `src`, within its [`FormLimits`].
pub fn read(src: &Source) -> Signatures {
    let pdf = src.pdf();
    let form = src.form();
    let mut r = Reader {
        pdf,
        limits: &src.limits().forms,
        names: 0,
    };
    let perms = (pdf.catalog().ok())
        .and_then(|c| c.get(b"Perms").ok())
        .and_then(|p| resolve(pdf, p))
        .and_then(|p| p.as_dict().ok());
    let perm = |key: &[u8]| perms.and_then(|p| p.get(key).ok());
    let certification = perm(b"DocMDP");
    let mut signed = Vec::new();
    let mut certified = false;
    for f in (form.fields.iter()).filter(|f| f.kind == FieldKind::Signature) {
        let Some(v) = value_of(pdf, form, f) else {
            continue;
        };
        let lock = (pdf.dictionary(f.object))
            .and_then(|d| d.get(b"Lock").ok())
            .map(|l| resolve(pdf, l).and_then(|l| l.as_dict().ok()));
        let certifies = certification.is_some_and(|c| same_object(c, v));
        let Some(sig) = r.signature(f.name.clone(), v, certifies, lock) else {
            continue;
        };
        certified |= sig.certifies;
        signed.push(sig);
    }
    if let Some(c) = certification
        && !certified
    {
        let sig = r.signature(String::new(), c, true, None);
        signed.push(sig.unwrap_or_else(|| Signature::unreadable(String::new(), true)));
    }
    // A damaged form keeps no fields: a signature it may have can't be
    // found, so one that can't be read stands for them.
    if form.facts.damaged.is_some()
        && signed.is_empty()
        && (form.facts.signed > 0 || form.facts.sig_flags & SIGNATURES_EXIST != 0)
    {
        signed.push(Signature::unreadable(String::new(), false));
    }
    let rights = [perm(b"UR3"), perm(b"UR")].into_iter().flatten();
    let usage_rights = (rights.filter_map(|s| resolve(pdf, s)?.as_dict().ok()))
        .next()
        .map(|sig| r.usage_rights(sig));
    Signatures {
        signed,
        usage_rights,
    }
}

/// Field `f`'s `/V`, its own or the nearest parent's; None when it has
/// none, or a null one.
fn value_of<'a>(pdf: &'a LazyPdf, form: &FileForm, f: &Field) -> Option<&'a Object> {
    let mut at = Some(f.id);
    // Up at most as many nodes as there are, should parents loop.
    for _ in 0..=form.nodes.len() {
        let node = form.nodes.get(at?.0 as usize)?;
        if let Some(v) = pdf.dictionary(node.object).and_then(|d| d.get(b"V").ok()) {
            return (!matches!(v, Object::Null)).then_some(v);
        }
        at = node.parent;
    }
    None
}

/// Whether `a` and `b` are references to the same object.
fn same_object(a: &Object, b: &Object) -> bool {
    matches!((a, b), (Object::Reference(a), Object::Reference(b)) if a == b)
}

/// `sig`'s `/Reference` entries, each None when it can't be read. A list
/// that can't be read at all, or holds more than an `/Opt` may, is one
/// entry that can't be.
fn references<'p>(
    pdf: &'p LazyPdf,
    sig: &'p Dictionary,
    limits: &FormLimits,
) -> Vec<Option<&'p Dictionary>> {
    let Ok(list) = sig.get(b"Reference") else {
        return Vec::new();
    };
    match resolve(pdf, list) {
        Some(Object::Array(items)) if items.len() <= limits.items => (items.iter())
            .map(|r| resolve(pdf, r).and_then(|r| r.as_dict().ok()))
            .collect(),
        _ => vec![None],
    }
}

/// What a signature's `/P` permits: 2 when `params` says none, and 1 when
/// it's past 1–3 or can't be read.
fn permits(p: Option<&Object>) -> u8 {
    match p.map(Object::as_i64) {
        None => 2,
        Some(Ok(p @ 1..=3)) => p as u8,
        Some(_) => 1,
    }
}

/// Reads signatures' dictionaries within the form caps: the names locks
/// list count as names, their lists as entries.
struct Reader<'a> {
    pdf: &'a LazyPdf,
    limits: &'a FormLimits,
    /// Bytes of lock names so far.
    names: usize,
}

impl Reader<'_> {
    /// The signature `v`, field `field`'s `/V`, with `lock` its field's
    /// `/Lock` (None inside when it can't be read). None when `v` is no
    /// dictionary, as a value a form's root passes down to every field
    /// may be: then the field isn't signed. One that can't be read, or
    /// lacks a `/ByteRange`, is a signature that can't be.
    fn signature(
        &mut self,
        field: String,
        v: &Object,
        certifies: bool,
        lock: Option<Option<&Dictionary>>,
    ) -> Option<Signature> {
        let pdf = self.pdf;
        let dict = match resolve(pdf, v) {
            Some(Object::Dictionary(d)) if d.has(b"ByteRange") => d,
            Some(Object::Dictionary(_)) | None => {
                return Some(Signature::unreadable(field, certifies));
            }
            Some(_) => return None,
        };
        let mut sig = Signature {
            field,
            p: None,
            certifies,
            locks: Vec::new(),
            unreadable: false,
        };
        // DocMDP's `/P`, when it has a DocMDP reference.
        let mut doc_mdp = None;
        for reference in references(pdf, dict, self.limits) {
            let Some(r) = reference else {
                sig.locks.push(Lock::All);
                continue;
            };
            let params = r.get(b"TransformParams").ok();
            let params = params.map(|p| resolve(pdf, p).and_then(|p| p.as_dict().ok()));
            match r.get(b"TransformMethod").and_then(Object::as_name) {
                Ok(b"DocMDP") => {
                    let p = match params {
                        None => 2,
                        Some(params) => params.map_or(1, |d| permits(d.get(b"P").ok())),
                    };
                    doc_mdp = Some(doc_mdp.map_or(p, |q: u8| q.min(p)));
                }
                Ok(b"FieldMDP") => sig.locks.push(self.lock(params.flatten())),
                _ => {}
            }
        }
        sig.certifies |= doc_mdp.is_some();
        if sig.certifies {
            // A certification without its reference allows nothing.
            sig.p = Some(doc_mdp.unwrap_or(1));
        }
        if let Some(lock) = lock {
            sig.locks.push(self.lock(lock));
            if let Some(p) = lock.and_then(|l| l.get(b"P").ok()) {
                let p = permits(Some(p));
                sig.p = Some(sig.p.map_or(p, |q| q.min(p)));
            }
        }
        Some(sig)
    }

    /// The fields `lock` locks, a FieldMDP reference's `/TransformParams`
    /// or a field's `/Lock`: all of them when it can't be read, or names
    /// an action it doesn't know or more names than the caps allow.
    fn lock(&mut self, lock: Option<&Dictionary>) -> Lock {
        let Some(d) = lock else {
            return Lock::All;
        };
        let names = || {
            let pdf = self.pdf;
            let list = resolve(pdf, d.get(b"Fields").ok()?)?.as_array().ok()?;
            if list.len() > self.limits.entries {
                return None;
            }
            let names: Option<Vec<String>> = (list.iter())
                .map(|n| resolve(pdf, n).filter(|n| n.as_str().is_ok()).map(pdf_text))
                .collect();
            let names = names?;
            let bytes: usize = names.iter().map(String::len).sum();
            let fits = names.iter().all(|n| n.len() <= self.limits.name)
                && self.names + bytes <= self.limits.names;
            fits.then_some((names, bytes))
        };
        let read = match d.get(b"Action").and_then(Object::as_name) {
            Ok(b"Include") => names().map(|(n, b)| (Lock::Include(n), b)),
            Ok(b"Exclude") => names().map(|(n, b)| (Lock::Exclude(n), b)),
            _ => None,
        };
        match read {
            Some((lock, bytes)) => {
                self.names += bytes;
                lock
            }
            None => Lock::All,
        }
    }

    /// What the usage rights signature `sig` grants, by its UR3
    /// reference's `/TransformParams`: nothing it can't read.
    fn usage_rights(&self, sig: &Dictionary) -> UsageRights {
        let pdf = self.pdf;
        let mut rights = UsageRights::default();
        for r in references(pdf, sig, self.limits).into_iter().flatten() {
            let method = r.get(b"TransformMethod").and_then(Object::as_name);
            if !matches!(method, Ok(b"UR3" | b"UR")) {
                continue;
            }
            let Some(params) = (r.get(b"TransformParams").ok())
                .and_then(|p| resolve(pdf, p))
                .and_then(|p| p.as_dict().ok())
            else {
                continue;
            };
            let names = |key: &[u8]| -> Vec<&[u8]> {
                let list = (params.get(key).ok())
                    .and_then(|l| resolve(pdf, l))
                    .and_then(|l| l.as_array().ok());
                (list.into_iter().flatten())
                    .filter_map(|n| resolve(pdf, n)?.as_name().ok())
                    .collect()
            };
            rights.fill_in |= names(b"Form").contains(&&b"FillIn"[..]);
            for name in names(b"Annots") {
                rights.annots |= match name {
                    b"Create" => AnnotOps::CREATE,
                    b"Modify" => AnnotOps::MODIFY,
                    b"Delete" => AnnotOps::DELETE,
                    _ => AnnotOps::empty(),
                };
            }
        }
        rights
    }
}

#[cfg(test)]
mod tests;
