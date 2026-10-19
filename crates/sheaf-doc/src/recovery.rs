//! Crash recovery (M10.5 §11.10). A document's unsaved state is kept in a
//! sidecar next to its file (`<name>.pdf.sheaf-recovery`), written a few
//! seconds after edits stop and whenever a save leaves edits after it.
//!
//! The sidecar is format 2:
//!
//! ```text
//! sheaf-recovery 2\n<append|full>\n<base len>\n<base stamp hex>\n<flags>\n<bytes>
//! ```
//!
//! - **The base** is the file as sheaf last read or wrote it
//!   ([`Document::file`](crate::Document::file)): its length, and its
//!   stamp ([`FileStamp::hex`]): the tail's hash, the hash at its last
//!   `startxref`, then the time. A sidecar is found only while its base is
//!   still the file, so it never outlives it.
//! - **Append** is sections after the base's bytes: what changed on the
//!   way from the saved version to the current one, written as an Append
//!   writes it (encrypted as the file is, save policy's form transforms
//!   applied, so a recovered fill of a hybrid form keeps no stale XFA),
//!   but whatever the planner would decide and with nothing stopping it:
//!   a sidecar is never the file, and may remove by omission. A restored
//!   document's sidecar is the sections it was restored with, then its
//!   own, against the same base (chained).
//! - **Full** is the whole document, the current version written as a
//!   save writes it, when no section can follow the base: its record says
//!   nothing of it (a Full sidecar's restore), it can't take an update,
//!   the objects a section refers to aren't all in it, or the legacy
//!   header and footer is drawn (until slice 17).
//! - **Flags** say what an Append sidecar's sections do, for the save that
//!   writes them: `removes`, when they remove or replace text the base has
//!   (by the path's effects, a transform that would need a Rewrite, or the
//!   removal check's find); `fill`, `annot` and `other`, their classes of
//!   change for the base's signatures; and `field:` with a field's full
//!   name in UTF-8 hex, for each field they touch. A Full sidecar's first
//!   save is a Rewrite, and it has none.
//!
//! A restored document opens from memory
//! ([`MemoryKind::Recovered`](crate::store::MemoryKind::Recovered)): the
//! base's bytes with the sections after them, read as one, or the whole
//! document. Its record is the file's with the sidecar pending
//! ([`SaveRecord::restored`]), so it isn't what the file holds until a
//! save writes the sections: its first save appends them, then its own,
//! unless they removed text (`Why::RecoveredRemovals`).

use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use in_progress::Progress;

use crate::effects::SigClass;
use crate::materialize::removal::Mode;
use crate::materialize::{
    self, Came, PendingTail, Profile, SaveInput, SaveRecord, SaveStop, SaveTarget, TailFlags,
};
use crate::serialize::durable::{self, FileStamp};
use crate::store::{BlobKind, Src, Store};

/// The first line of a sidecar in this format.
const FORMAT: &[u8] = b"sheaf-recovery 2\n";
pub const SUFFIX: &str = ".sheaf-recovery";

/// The flag of sections that remove or replace text their base has.
const REMOVES: &str = "removes";

/// The flags of the sections' classes of change.
const CLASSES: [(&str, SigClass); 3] = [
    ("fill", SigClass::FILL),
    ("annot", SigClass::ANNOT),
    ("other", SigClass::OTHER),
];

/// What a flag naming a field the sections touch starts with: its full
/// name follows, in UTF-8 hex.
const FIELD: &str = "field:";

pub fn sidecar_path(pdf: &Path) -> PathBuf {
    let mut name = pdf
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    name.push(SUFFIX);
    pdf.with_file_name(name)
}

/// Write the sidecar of `input`, a document's state ([`recovery_input`]),
/// beside `file`, the document's file, which `base` stamps: Append
/// sections after it when they can follow it, or else the Full form.
/// Returns its size: 0 when sections that change nothing leave no sidecar
/// (and any there goes).
///
/// [`recovery_input`]: crate::Document::recovery_input
pub fn write(input: &SaveInput, file: &Path, base: &FileStamp, p: &dyn Progress) -> Result<usize> {
    let stamp = base.hex().context("the file's time is before 1970")?;
    let target = SaveTarget {
        path: file.into(),
        same_file: true,
        profile: Profile::default(),
        stamp: None,
    };
    let (kind, flags, body) = match sections(input, &target, base, p)? {
        Some((_, sections)) if sections.is_empty() => {
            discard(file);
            return Ok(0);
        }
        Some((flags, sections)) => ("append", words(&flags), sections),
        None => {
            // A Rewrite's plan, for its transforms: a sidecar asks nothing,
            // and is never the saved file.
            let plan = materialize::forced_rewrite(input, &target);
            let out = materialize::materialize(input, &plan, target, p)?;
            ("full", String::new(), out.bytes)
        }
    };
    let head = format!("{kind}\n{}\n{stamp}\n{flags}\n", base.len);
    let mut bytes = Vec::with_capacity(FORMAT.len() + head.len() + body.len());
    bytes.extend_from_slice(FORMAT);
    bytes.extend_from_slice(head.as_bytes());
    bytes.extend_from_slice(&body);
    durable::write(&sidecar_path(file), &bytes, None)?;
    Ok(bytes.len())
}

/// The sections a sidecar of `input` holds after the saved file, at
/// `target`'s path as `base` stamps it, with their flags: a restored
/// document's pending ones, then its own, which the removal check and the
/// signatures' guard flag rather than stop. None when no section can
/// follow the file, so the sidecar is Full: one refused, or one whose
/// making failed, so a fault in it never leaves a document without a
/// sidecar. A file changed on disk, or a cancel, stops the sidecar.
fn sections(
    input: &SaveInput,
    target: &SaveTarget,
    base: &FileStamp,
    p: &dyn Progress,
) -> Result<Option<(TailFlags, Vec<u8>)>> {
    let of_base = |record: &SaveRecord| *record.path == *target.path && record.stamp == *base;
    if !(input.saved.as_ref()).is_some_and(|saved| of_base(&saved.record)) {
        return Ok(None);
    }
    let Some((plan, mut flags)) = materialize::plan::sidecar(input) else {
        return Ok(None);
    };
    match materialize::append_onto_saved(input, &plan, target, Mode::Recovery, p) {
        Ok(Ok(out)) => {
            flags.removes |= out.unjudged.stays_readable;
            // Signatures allow no change of the catalog or the form's
            // dictionary after them.
            if out.unjudged.writes_roots {
                flags.sig |= SigClass::OTHER;
            }
            Ok(Some((flags, out.bytes)))
        }
        Ok(Err(refused)) => {
            log::debug!(
                "the recovery sidecar of {} is Full: {refused}",
                target.path.display()
            );
            Ok(None)
        }
        Err(stop @ (SaveStop::Ask(_) | SaveStop::Cancelled(_))) => Err(stop.into()),
        Err(SaveStop::Failed(why)) => {
            log::warn!(
                "the recovery sidecar of {} is Full, its section failed: {why}",
                target.path.display()
            );
            Ok(None)
        }
    }
}

/// `flags` as a sidecar says them.
fn words(flags: &TailFlags) -> String {
    let mut words: Vec<String> = Vec::new();
    if flags.removes {
        words.push(REMOVES.into());
    }
    for (word, class) in CLASSES {
        if flags.sig.contains(class) {
            words.push(word.into());
        }
    }
    for name in &flags.fields {
        let hex: String = name.bytes().map(|b| format!("{b:02x}")).collect();
        words.push(format!("{FIELD}{hex}"));
    }
    words.join(" ")
}

/// The flags `line` says, when it says nothing else.
fn flags_of(line: &str) -> Option<TailFlags> {
    let mut flags = TailFlags::default();
    for word in line.split_whitespace() {
        if word == REMOVES {
            flags.removes = true;
        } else if let Some(&(_, class)) = CLASSES.iter().find(|(w, _)| *w == word) {
            flags.sig |= class;
        } else {
            let hex = (word.strip_prefix(FIELD))
                .filter(|hex| hex.len() % 2 == 0 && hex.bytes().all(|b| b.is_ascii_hexdigit()))?;
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).ok())
                .collect::<Option<Vec<u8>>>()?;
            flags.fields.insert(String::from_utf8(bytes).ok()?);
        }
    }
    Some(flags)
}

/// Recovery data found next to a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Recovery {
    pub sidecar: PathBuf,
    /// The file it was written against, which it's found only beside.
    pub base: FileStamp,
    /// What follows its head: sections after the base's bytes, or the
    /// whole document (Full).
    pub bytes: Arc<[u8]>,
    /// What a document restored from it holds that its base hasn't got,
    /// for its first save: these sections (none when Full) and what they
    /// do.
    pub pending: PendingTail,
}

/// A usable sidecar for `pdf`: present, well-formed, and written against
/// the file as it still is (its length and stamp). A stale or malformed
/// one is removed; one whose file can't be read now is left.
pub fn find(pdf: &Path) -> Option<Recovery> {
    let sidecar = sidecar_path(pdf);
    let bytes = std::fs::read(&sidecar).ok()?;
    let found = parse(&bytes).map(|(base, whole, flags, body)| {
        let body: Arc<[u8]> = body.into();
        Recovery {
            sidecar: sidecar.clone(),
            base,
            pending: PendingTail {
                bytes: (!whole).then(|| body.clone()),
                flags,
            },
            bytes: body,
        }
    });
    match found.as_ref().map(|rec| rec.base.holds(pdf)) {
        Some(Ok(true)) => found,
        Some(Err(_)) => None,
        Some(Ok(false)) | None => {
            let _ = std::fs::remove_file(&sidecar);
            None
        }
    }
}

/// The sidecar's base, whether it's Full, its flags and its bytes, when
/// it's this format and says nothing it doesn't know.
fn parse(bytes: &[u8]) -> Option<(FileStamp, bool, TailFlags, &[u8])> {
    let mut rest = bytes.strip_prefix(FORMAT)?;
    let mut line = || {
        let nl = rest.iter().position(|&b| b == b'\n')?;
        let line = std::str::from_utf8(&rest[..nl]).ok()?;
        rest = &rest[nl + 1..];
        Some(line)
    };
    let whole = match line()? {
        "append" => false,
        "full" => true,
        _ => return None,
    };
    let len: u64 = line()?.parse().ok()?;
    let base = FileStamp::from_hex(len, line()?)?;
    let flags = flags_of(line()?)?;
    (!rest.is_empty()).then_some((base, whole, flags, rest))
}

/// The document `rec` recovers for `pdf`, opened from memory into `store`
/// with `password`, while the file is still its base: the file's bytes,
/// read once and checked against the base's stamp, with the sidecar's
/// sections after them, or a Full sidecar's document; the file's name,
/// which it goes by; and how it came, for its record
/// ([`SaveRecord::restored`]): the file, the base, and what the sidecar
/// holds that the file hasn't got.
pub fn restore(
    store: &Store,
    pdf: &Path,
    rec: &Recovery,
    password: Option<&str>,
) -> Result<(Src, Arc<str>, Came)> {
    let reading = || format!("reading {}", pdf.display());
    let changed = || {
        format!(
            "{} changed since the recovery data was written",
            pdf.display()
        )
    };
    let bytes: Arc<[u8]> = match &rec.pending.bytes {
        Some(sections) => {
            let mut file = File::open(pdf).with_context(reading)?;
            let meta = file.metadata().with_context(reading)?;
            let mut bytes = Vec::with_capacity(meta.len() as usize + sections.len());
            (&mut file)
                .take(meta.len())
                .read_to_end(&mut bytes)
                .with_context(reading)?;
            if FileStamp::of(&meta, &bytes).with_context(reading)? != rec.base {
                bail!(changed());
            }
            bytes.extend_from_slice(sections);
            bytes.into()
        }
        None => {
            if !rec.base.holds(pdf).with_context(reading)? {
                bail!(changed());
            }
            rec.bytes.clone()
        }
    };
    let src = (store.open(bytes, BlobKind::recovered(password)))
        .with_context(|| format!("opening the recovery data of {}", pdf.display()))?;
    let came = Came::Restored {
        path: pdf.into(),
        base: rec.base,
        pending: rec.pending.clone(),
    };
    Ok((src, crate::store::file_name(pdf), came))
}

/// Remove the sidecar, if any.
pub fn discard(pdf: &Path) {
    let _ = std::fs::remove_file(sidecar_path(pdf));
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use in_progress::NoProgress;
    use lopdf::{Document as Pdf, LazyPdf, Object, Stream, dictionary};
    use sheaf_edit::Rotation;

    use super::*;
    use crate::effects::{Removed, RemovedKind};
    use crate::forms::fixture::{Form, row};
    use crate::materialize::{
        AppendAnyway, Ask, AskAnswers, Onto, SaveStop, Strategy, StrategyKind, Why, plan, save,
    };
    use crate::model::Document;
    use crate::serialize::fixture::holds;
    use crate::source::Limits;
    use crate::store::{MemoryKind, Origin};
    use crate::tx::fixture as fx;
    use sheaf_edit::{AnnotRef, LabelStyle, Op, Turn};

    /// An empty directory of the test's own.
    fn scratch() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("sheaf-recovery-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn two_page_pdf(path: &Path) {
        let mut pdf = Pdf::with_version("1.7");
        let pages_id = pdf.new_object_id();
        let mut kids = Vec::new();
        for _ in 0..2 {
            let content = pdf.add_object(Stream::new(dictionary! {}, b"".to_vec()));
            let page = pdf.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id,
                "MediaBox" => Object::Array(vec![0.into(), 0.into(), 612.into(), 792.into()]),
                "Contents" => content,
            });
            kids.push(Object::Reference(page));
        }
        pdf.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 2 }),
        );
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        pdf.trailer.set("Root", catalog);
        pdf.save(path).unwrap();
    }

    /// `doc`'s sidecar, written as the app writes it: beside its file,
    /// against the file as sheaf last read or wrote it.
    fn sidecar_of(doc: &Document) -> usize {
        let (file, base) = doc.file().map(|(f, s)| (f.clone(), *s)).unwrap();
        write(&doc.recovery_input(), &file, &base, &NoProgress).unwrap()
    }

    /// The document `rec` restores for `pdf`, opened as the app opens it.
    pub(crate) fn restored(pdf: &Path, rec: &Recovery, password: Option<&str>) -> Document {
        let store = Store::new();
        let (src, name, came) = restore(&store, pdf, rec, password).unwrap();
        let policy = crate::history::default_policy();
        let mut doc = crate::open::start(store, src.clone(), name, Some(came), policy);
        doc.ready(crate::open::finish(&src, &NoProgress)).unwrap();
        doc
    }

    /// The file at `pdf` with `rec`'s sections after it, as one.
    fn with_sections(pdf: &Path, rec: &Recovery) -> Vec<u8> {
        let sections = rec.pending.bytes.as_ref().expect("an Append sidecar");
        [std::fs::read(pdf).unwrap(), sections.to_vec()].concat()
    }

    /// What the sidecar beside `pdf` says it is: its second line.
    fn kind_of(pdf: &Path) -> String {
        let bytes = std::fs::read(sidecar_path(pdf)).unwrap();
        let rest = bytes.strip_prefix(FORMAT).unwrap();
        let nl = rest.iter().position(|&b| b == b'\n').unwrap();
        String::from_utf8(rest[..nl].to_vec()).unwrap()
    }

    fn turn(doc: &mut Document, index: usize) {
        let page = doc.page(index).unwrap();
        doc.commit_ops(vec![fx::rotate(&[page], Turn::Quarter)], "rot")
            .unwrap();
    }

    /// Annotation `id` deleted.
    fn delete(id: crate::annot::AnnotId) -> Op {
        Op::DeleteAnnotations {
            annots: vec![AnnotRef::Id(id)],
        }
    }

    /// A sidecar holds what the edits changed, as sections after its file,
    /// against that file: found while the file is still that one, and
    /// restored from memory as the file and the sections read as one, its
    /// record the file's with the sections pending, so it isn't what the
    /// file holds. It's stale once another program changed the file, when
    /// it goes. A malformed one goes, and so does one of the format before.
    #[test]
    fn sidecar_round_trips_and_detects_staleness() {
        let dir = scratch();
        let pdf = dir.join("doc.pdf");
        two_page_pdf(&pdf);
        let original = std::fs::read(&pdf).unwrap();
        let mut model = crate::open::file(&pdf, None).unwrap().doc;
        turn(&mut model, 1);
        let n = sidecar_of(&model);
        assert_eq!(
            std::fs::metadata(sidecar_path(&pdf)).unwrap().len() as usize,
            n
        );
        assert_eq!(kind_of(&pdf), "append");
        // The original is untouched.
        assert_eq!(std::fs::read(&pdf).unwrap(), original);

        let rec = find(&pdf).expect("sidecar found");
        assert_eq!(rec.base, FileStamp::read(&pdf).unwrap());
        assert_eq!(rec.base.len, original.len() as u64);
        assert_eq!(rec.pending.bytes.as_ref(), Some(&rec.bytes));
        // A page turned is another class of change than a fill or a
        // comment, and removes nothing.
        let flags = TailFlags {
            removes: false,
            sig: SigClass::OTHER,
            fields: Default::default(),
        };
        assert_eq!(rec.pending.flags, flags);
        let recovered = restored(&pdf, &rec, None);
        assert!(recovered.is_dirty() && !recovered.can_undo());
        let record = recovered.saved_record().unwrap();
        assert_eq!(record.pending_tail.as_ref(), Some(&rec.pending));
        assert_eq!(
            recovered.file().map(|(f, s)| (&**f, *s)),
            Some((pdf.as_path(), rec.base))
        );
        let primary = recovered.primary();
        assert_eq!(
            **recovered.name(),
            *pdf.file_name().unwrap().to_string_lossy()
        );
        assert_eq!(**primary.bytes(), *with_sections(&pdf, &rec));
        let origin = match primary.blob().kind() {
            BlobKind::Pdf { origin, .. } => origin.clone(),
            BlobKind::Attachment => unreachable!("a PDF"),
        };
        assert_eq!(origin, Origin::Memory(MemoryKind::Recovered));
        assert_eq!(primary.geom(1).rotate, Rotation::R90);
        assert_eq!(primary.geom(0).rotate, Rotation::R0);
        assert_eq!(
            recovered.geom(recovered.page(1).unwrap()).rotate,
            Rotation::R90
        );
        // Nothing of it in a temp file of its own.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);

        // The file changed underneath: the sidecar is stale and goes away.
        std::fs::write(&pdf, [original.clone(), b"\n%extra\n".to_vec()].concat()).unwrap();
        assert!(find(&pdf).is_none());
        assert!(!sidecar_path(&pdf).exists());
        // So does one whose file changed after it was found.
        std::fs::write(&pdf, &original).unwrap();
        model = crate::open::file(&pdf, None).unwrap().doc;
        turn(&mut model, 0);
        sidecar_of(&model);
        let rec = find(&pdf).unwrap();
        std::fs::write(&pdf, [original.clone(), b"\n".to_vec()].concat()).unwrap();
        let store = Store::new();
        assert!(restore(&store, &pdf, &rec, None).is_err());

        // A malformed sidecar, or one of the format before, goes.
        std::fs::write(&pdf, &original).unwrap();
        let base = FileStamp::read(&pdf).unwrap();
        let v1 = format!("sheaf-recovery 1\n{}\n%%section", original.len());
        let head = |kind: &str, flags: &str| {
            let stamp = base.hex().unwrap();
            format!(
                "sheaf-recovery 2\n{kind}\n{}\n{stamp}\n{flags}\n%PDF-",
                base.len
            )
        };
        for bad in [
            v1,
            head("full", "removes and more"),
            head("append", "field:6"),
            head("append", "field:zz"),
            head("some", ""),
            head("full", "")[..40].to_string(),
        ] {
            std::fs::write(sidecar_path(&pdf), bad.as_bytes()).unwrap();
            assert!(find(&pdf).is_none(), "{bad}");
            assert!(!sidecar_path(&pdf).exists(), "{bad}");
        }
        std::fs::write(sidecar_path(&pdf), head("full", "").as_bytes()).unwrap();
        let rec = find(&pdf).unwrap();
        assert!(rec.pending.bytes.is_none() && *rec.bytes == *b"%PDF-");
        let said = "removes annot other field:612e62 field:c3a9";
        std::fs::write(sidecar_path(&pdf), head("append", said).as_bytes()).unwrap();
        let rec = find(&pdf).unwrap();
        let flags = TailFlags {
            removes: true,
            sig: SigClass::ANNOT | SigClass::OTHER,
            fields: ["a.b", "é"].map(String::from).into(),
        };
        assert_eq!(rec.pending.flags, flags);
        assert_eq!(words(&flags), said);
        assert_eq!(rec.pending.bytes.as_deref(), Some(b"%PDF-".as_slice()));

        discard(&pdf);
        discard(&pdf); // no-op without a sidecar
        assert!(!sidecar_path(&pdf).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A sidecar is stale once its file changed, even at the same length
    /// and time: a change in the file's tail shows, and so does one at its
    /// last `startxref`'s target, further back than the tail, as a long
    /// cross-reference table's is. `find` drops it, and `restore` refuses
    /// it.
    #[test]
    fn a_same_length_change_makes_a_sidecar_stale() {
        let dir = scratch();
        let pdf = dir.join("long.pdf");
        let bytes = crate::serialize::fixture::blank_table(3500);
        let (at, _) = durable::startxref(&bytes).unwrap();
        assert!(bytes.len() as u64 - at > durable::TAIL, "far back");
        // A byte in the tail, and one in the cross-reference table's first
        // row, past its `xref` line.
        for change in [bytes.len() - 10, at as usize + 12] {
            std::fs::write(&pdf, &bytes).unwrap();
            let mut model = crate::open::file(&pdf, None).unwrap().doc;
            turn(&mut model, 0);
            sidecar_of(&model);
            let rec = find(&pdf).expect("found while the file is its base");
            let mtime = std::fs::metadata(&pdf).unwrap().modified().unwrap();
            let mut changed = bytes.clone();
            changed[change] ^= 1;
            std::fs::write(&pdf, &changed).unwrap();
            let file = std::fs::File::options().write(true).open(&pdf).unwrap();
            file.set_modified(mtime).unwrap();
            drop(file);
            let store = Store::new();
            assert!(restore(&store, &pdf, &rec, None).is_err(), "{change}");
            assert!(find(&pdf).is_none(), "{change}");
            assert!(!sidecar_path(&pdf).exists(), "{change}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A sidecar of one mark on a long file holds the mark and what it
    /// changed, not the file: far smaller than it.
    #[test]
    fn an_append_sidecar_of_one_mark_is_far_smaller_than_the_file() {
        let dir = scratch();
        let pdf = dir.join("long.pdf");
        std::fs::write(&pdf, crate::serialize::fixture::blank_table(3500)).unwrap();
        let mut model = crate::open::file(&pdf, None).unwrap().doc;
        crate::serialize::tests::mark(&mut model);
        let n = sidecar_of(&model);
        let len = std::fs::metadata(&pdf).unwrap().len() as usize;
        assert_eq!(kind_of(&pdf), "append");
        assert!(n * 20 < len, "{n} of {len}");
        let rec = find(&pdf).unwrap();
        assert_eq!(rec.pending.flags.sig, SigClass::ANNOT);
        let recovered = restored(&pdf, &rec, None);
        assert_eq!(
            recovered
                .state()
                .marks_on(recovered.page(0).unwrap())
                .count(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A section that fails to be made for anything but a refusal (here a
    /// record whose map has lost the catalog, which no Append can follow)
    /// leaves a Full sidecar, not none: a fault in the Append never takes
    /// a document's crash protection with it.
    #[test]
    fn a_section_that_fails_leaves_a_full_sidecar() {
        let dir = scratch();
        let pdf = dir.join("doc.pdf");
        two_page_pdf(&pdf);
        let mut model = crate::open::file(&pdf, None).unwrap().doc;
        turn(&mut model, 0);
        let mut input = model.recovery_input();
        let saved = input.saved.as_mut().unwrap();
        let mut record = SaveRecord::clone(&saved.record);
        Arc::make_mut(&mut record.map)
            .keys
            .remove(&crate::materialize::ObjKey::Catalog);
        saved.record = Arc::new(record);
        let (file, base) = model.file().map(|(f, s)| (f.clone(), *s)).unwrap();
        assert!(write(&input, &file, &base, &NoProgress).unwrap() > 0);
        assert_eq!(kind_of(&pdf), "full");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sidecar_carries_ocr_layers() {
        use crate::annot::Box2;
        use crate::text_layer::OcrWord;
        let dir = scratch();
        let pdf = dir.join("doc.pdf");
        two_page_pdf(&pdf);
        let mut model = crate::open::file(&pdf, None).unwrap().doc;
        let p1 = model.page(1).unwrap();
        let layer = crate::text_layer::from_engine(
            vec![OcrWord {
                text: "Hello".into(),
                rect: Box2 {
                    x: 72.0,
                    y: 100.0,
                    w: 100.0,
                    h: 12.0,
                },
                confidence: 1.0,
            }],
            &crate::geom::Geom::read(None, None, 0),
            "test",
        );
        model
            .commit_ops(fx::ocr([(p1, Arc::new(layer))]), "ocr")
            .unwrap();
        sidecar_of(&model);
        let rec = find(&pdf).expect("sidecar found");
        let recovered = Pdf::load_mem(&with_sections(&pdf, &rec)).unwrap();
        let pages = recovered.get_pages();
        let p2 = recovered.get_dictionary(pages[&2]).unwrap();
        let contents = recovered
            .dereference(p2.get(b"Contents").unwrap())
            .unwrap()
            .1
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(contents.len(), 3);
        let text = String::from_utf8_lossy(&recovered.get_page_content(pages[&2])).into_owned();
        assert!(
            text.contains("3 Tr") && text.contains("<00480065006C006C006F>"),
            "{text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A page deleted from a hybrid form changes its form, so a sidecar,
    /// as any save, takes the XFA part out (save policy, `rules::plan`): a
    /// recovered document has no stale XFA. The deleted page's field value
    /// stays in the file's revision before the sections, so the sidecar
    /// says they remove, and the restored document's first save rewrites
    /// the file for it, which has neither.
    #[test]
    fn a_page_delete_on_a_hybrid_form_gives_a_sidecar_with_no_xfa() {
        let dir = scratch();
        let pdf = dir.join("form.pdf");
        let mut f = Form::new(2);
        let x = f.text("x[0]", 0, row(0));
        let y = f.text("y[0]", 1, row(0));
        f.fill(x, "Ex");
        f.fill(y, "Why");
        f.group("form1[0]", &[x, y]);
        f.xfa();
        std::fs::write(&pdf, f.bytes()).unwrap();
        let mut model = crate::open::file(&pdf, None).unwrap().doc;
        assert!(model.state().form().xfa.present);
        let second = model.page(1).unwrap();
        let delete = Op::DeletePages {
            pages: fx::ids(&[second]),
        };
        model.commit_ops(vec![delete], "delete").unwrap();
        sidecar_of(&model);
        let sidecar = std::fs::read(sidecar_path(&pdf)).unwrap();
        assert!(!holds(&sidecar, b"xfa-template"));
        let rec = find(&pdf).unwrap();
        assert!(rec.pending.flags.removes);
        let bytes = with_sections(&pdf, &rec);
        let src = crate::forms::fixture::source(bytes.clone());
        assert!(!src.form().facts.xfa);
        let report = crate::forms::check_latest(&bytes, None).unwrap();
        let earlier = crate::forms::Finding::EarlierValues {
            revision: 1,
            names: vec!["form1[0].y[0]".into()],
        };
        assert_eq!(report.findings, [earlier]);
        // The file itself keeps its XFA: a sidecar is never the file.
        assert!(
            crate::forms::fixture::source(std::fs::read(&pdf).unwrap())
                .form()
                .facts
                .xfa
        );
        let mut recovered = restored(&pdf, &rec, None);
        let report = save(&mut recovered, &pdf, &AskAnswers::default(), &NoProgress).unwrap();
        assert_eq!(report.why, [Why::RecoveredRemovals]);
        let saved = std::fs::read(&pdf).unwrap();
        assert!(
            !crate::forms::fixture::source(saved.clone())
                .form()
                .facts
                .xfa
        );
        let report = crate::forms::check(&saved, None).unwrap();
        assert!(report.is_clean(), "{:?}", report.findings);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The sidecar of an encrypted file opened without a password is
    /// encrypted as the file is: neither the page's string (the turned
    /// page's dictionary, which its section writes again) nor a mark's
    /// note is in it in the clear, and it restores, encrypted, with the
    /// turn and the mark.
    #[test]
    fn an_encrypted_sidecar_holds_no_plaintext() {
        use std::collections::BTreeMap;

        use lopdf::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter};
        use lopdf::{EncryptionState, EncryptionVersion, Permissions, XrefKind};

        use crate::serialize::fixture::{OWNER, encrypted, whole};
        use crate::serialize::tests::{NOTE, mark, notes};
        use crate::source::lazy_tests::{PAGE_STRING, plaintext};

        let dir = scratch();
        let plain = plaintext();
        // Opened with its empty user password, as such files open.
        let state = EncryptionState::try_from(EncryptionVersion::V4 {
            document: &plain,
            encrypt_metadata: true,
            crypt_filters: BTreeMap::from([(
                b"StdCF".to_vec(),
                Arc::new(Aes128CryptFilter) as Arc<dyn CryptFilter>,
            )]),
            stream_filter: b"StdCF".to_vec(),
            string_filter: b"StdCF".to_vec(),
            owner_password: OWNER,
            user_password: "",
            permissions: Permissions::all(),
        })
        .unwrap();
        let [table, _] = encrypted(&plain, &state, |_| {});
        let unencrypted = whole(&plain.objects, &plain.trailer, XrefKind::Table, "1.7", None);
        for (encrypted, bytes) in [(false, unencrypted), (true, table)] {
            let pdf = dir.join(format!("encrypted-{encrypted}.pdf"));
            std::fs::write(&pdf, &bytes).unwrap();
            let mut model = crate::open::file(&pdf, None).unwrap().doc;
            turn(&mut model, 0);
            mark(&mut model);
            sidecar_of(&model);
            assert_eq!(kind_of(&pdf), "append");
            // The turned page's string and the mark's note, in the clear
            // only when the file is.
            let sidecar = std::fs::read(sidecar_path(&pdf)).unwrap();
            for text in [PAGE_STRING, NOTE] {
                assert_eq!(holds(&sidecar, text), !encrypted, "{encrypted}");
            }
            let rec = find(&pdf).expect("sidecar found");
            let bytes: Arc<[u8]> = with_sections(&pdf, &rec).into();
            let lazy = LazyPdf::open(bytes, None, &Limits::DEFAULT.reader).unwrap();
            assert_eq!(lazy.is_encrypted(), encrypted);
            assert_eq!(notes(&lazy), [NOTE], "{encrypted}");
            let recovered = restored(&pdf, &rec, None);
            let page = recovered.page(0).unwrap();
            assert_eq!(recovered.geom(page).rotate, Rotation::R90);
            assert_eq!(recovered.state().marks_on(page).count(), 1);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A password-protected document gets a sidecar too, encrypted with
    /// the file's keys: neither the mark's note nor the form's value is in
    /// it in the clear. It restores with the password, and not without.
    #[test]
    fn a_password_protected_document_s_sidecar_restores_with_the_password() {
        use crate::serialize::fixture::{USER, VALUE, edited, encrypted, form};
        use crate::serialize::tests::NOTE;

        let dir = scratch();
        let plain = form();
        for (name, state) in crate::serialize::fixture::rc4_aes_128_and_aes_256(&plain) {
            let [table, _] = encrypted(&plain, &state, |_| {});
            let pdf = dir.join(format!("{name}.pdf"));
            std::fs::write(&pdf, &table).unwrap();
            let mut model = edited(&pdf, Some(USER));
            crate::serialize::tests::mark(&mut model);
            sidecar_of(&model);
            assert_eq!(kind_of(&pdf), "append", "{name}");
            let sidecar = std::fs::read(sidecar_path(&pdf)).unwrap();
            for text in [NOTE, VALUE.as_bytes()] {
                assert!(!holds(&sidecar, text), "{name}");
            }
            let rec = find(&pdf).unwrap();
            let store = Store::new();
            assert!(restore(&store, &pdf, &rec, None).is_err(), "{name}");
            let recovered = restored(&pdf, &rec, Some(USER));
            let page = recovered.page(0).unwrap();
            assert_eq!(recovered.geom(page).rotate, Rotation::R90, "{name}");
            assert_eq!(recovered.state().marks_on(page).count(), 2, "{name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The header and footer is drawn by a Rewrite until slice 17, so the
    /// sidecar of a document with an unsaved one is Full: the whole
    /// document, encrypted as the file is, the header's text nowhere in it
    /// in the clear. It restores with a record that says nothing of the
    /// file, so its first save is a Rewrite.
    #[test]
    fn an_encrypted_file_s_full_sidecar_holds_no_plaintext() {
        use crate::serialize::fixture::{USER, VALUE, encrypted, form};

        let dir = scratch();
        let plain = form();
        let [(_, state), ..] = crate::serialize::fixture::rc4_aes_128_and_aes_256(&plain);
        let [table, _] = encrypted(&plain, &state, |_| {});
        let pdf = dir.join("header.pdf");
        std::fs::write(&pdf, &table).unwrap();
        let mut model = crate::open::file(&pdf, Some(USER)).unwrap().doc;
        let set = fx::set_header(fx::header("HEADERSECRET <<1>>"), None);
        model.commit_ops(vec![set], "header").unwrap();
        sidecar_of(&model);
        assert_eq!(kind_of(&pdf), "full");
        let sidecar = std::fs::read(sidecar_path(&pdf)).unwrap();
        for text in [b"HEADERSECRET".as_slice(), VALUE.as_bytes()] {
            assert!(!holds(&sidecar, text));
        }
        let rec = find(&pdf).unwrap();
        assert!(rec.pending.bytes.is_none());
        let recovered = restored(&pdf, &rec, Some(USER));
        assert!(recovered.is_dirty());
        assert!(!recovered.saved_record().unwrap().knows_file());
        let hf = recovered.state().header_footer().unwrap();
        assert_eq!(hf.texts[0], "HEADERSECRET <<1>>");
        let planned = plan(&recovered.recovery_input(), &recovered.save_target(&pdf));
        let why = vec![Why::NoSaveRecord, Why::LegacyHeaderFooter];
        assert_eq!(planned.strategy, Strategy::Rewrite { why });
        // Its own sidecar is Full too, against the same file.
        sidecar_of(&recovered);
        assert_eq!(kind_of(&pdf), "full");
        let again = restored(&pdf, &find(&pdf).unwrap(), Some(USER));
        let hf = again.state().header_footer().unwrap();
        assert_eq!(hf.texts[0], "HEADERSECRET <<1>>");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What the planner misjudges, a sidecar's section still flags: a
    /// deleted mark judged as removing nothing leaves the mark's text in
    /// what the section unlinks, which the removal check finds; page
    /// labels judged as a comment write the catalog, which no signature
    /// allows after it.
    #[test]
    fn a_misjudged_section_still_flags_its_sidecar() {
        use crate::effects::{Flags, PathEffects, misjudged};

        let dir = scratch();
        let pdf = dir.join("doc.pdf");
        two_page_pdf(&pdf);
        let mut model = crate::open::file(&pdf, None).unwrap().doc;
        crate::serialize::tests::mark(&mut model);
        let report = save(&mut model, &pdf, &AskAnswers::default(), &NoProgress).unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Append));
        let page = model.page(0).unwrap();
        let id = model.state().marks_on(page).next().unwrap().id;
        model.commit_ops(vec![delete(id)], "delete").unwrap();
        fn removes_nothing(path: &mut PathEffects) {
            path.flags.remove(Flags::REMOVES | Flags::REPLACES_TEXT);
            path.removed.clear();
        }
        misjudged(removes_nothing, || sidecar_of(&model));
        assert!(find(&pdf).unwrap().pending.flags.removes);

        model.undo().unwrap();
        let labels = Op::SetPageLabels {
            ranges: Some(vec![fx::label(page, LabelStyle::LowerRoman, "", 1)]),
        };
        model.commit_ops(vec![labels], "labels").unwrap();
        fn comments_alone(path: &mut PathEffects) {
            path.sig = SigClass::ANNOT;
        }
        misjudged(comments_alone, || sidecar_of(&model));
        let flags = find(&pdf).unwrap().pending.flags;
        assert_eq!(flags.sig, SigClass::ANNOT | SigClass::OTHER);
        assert!(!flags.removes);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A second crash chains: a restored document's sidecar is the sections
    /// it was restored with, then its own, against the same file, and
    /// they restore together. Its first save then writes them all after the
    /// file, which reopens with every edit.
    #[test]
    fn a_second_crash_chains_the_sections() {
        let dir = scratch();
        let pdf = dir.join("doc.pdf");
        two_page_pdf(&pdf);
        let original = std::fs::read(&pdf).unwrap();
        let mut model = crate::open::file(&pdf, None).unwrap().doc;
        turn(&mut model, 0);
        sidecar_of(&model);
        let first = find(&pdf).unwrap();
        let mut again = restored(&pdf, &first, None);
        // Restored and not edited, its sidecar is what it was restored
        // with.
        sidecar_of(&again);
        assert_eq!(find(&pdf).unwrap(), first);
        crate::serialize::tests::mark(&mut again);
        sidecar_of(&again);
        let second = find(&pdf).unwrap();
        assert_eq!(second.base, first.base);
        assert!(second.bytes.starts_with(&first.bytes));
        assert!(second.bytes.len() > first.bytes.len());
        assert_eq!(second.pending.flags.sig, SigClass::OTHER | SigClass::ANNOT);
        let mut third = restored(&pdf, &second, None);
        let page = third.page(0).unwrap();
        assert_eq!(third.geom(page).rotate, Rotation::R90);
        assert_eq!(third.state().marks_on(page).count(), 1);
        let report = save(&mut third, &pdf, &AskAnswers::default(), &NoProgress).unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Append));
        assert!(!third.is_dirty());
        let written = std::fs::read(&pdf).unwrap();
        assert!(written.starts_with(&original));
        assert!(written[original.len()..].starts_with(&second.bytes));
        let reopened = crate::open::file(&pdf, None).unwrap().doc;
        let page = reopened.page(0).unwrap();
        assert_eq!(reopened.geom(page).rotate, Rotation::R90);
        assert_eq!(reopened.state().marks_on(page).count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A restored signed file's first save appends what the sidecar holds
    /// when its sections removed nothing: a comment, which its approval
    /// signature allows after it, asks nothing, and the signed bytes are
    /// the file's first. When they removed something, a comment deleted,
    /// it's a Rewrite for that, which asks before it breaks the signature,
    /// offering an Append anyway that leaves what they removed readable;
    /// taken, the signed bytes are the file's first still.
    #[test]
    fn a_restored_signed_file_appends_unless_its_sections_removed() {
        let dir = scratch();
        let mut f = Form::new(1);
        f.text("t", 0, row(0));
        let sig = f.signature("sig", 0, row(1));
        f.sign(sig);
        let bytes = f.bytes();
        let pdf = dir.join("signed.pdf");
        std::fs::write(&pdf, &bytes).unwrap();
        let fits = |bytes: &[u8]| {
            let findings = crate::forms::check(bytes, None).unwrap().findings;
            !(findings.iter()).any(|f| matches!(f, crate::forms::Finding::StaleSignature { .. }))
        };
        let mut model = crate::open::file(&pdf, None).unwrap().doc;
        crate::serialize::tests::mark(&mut model);
        sidecar_of(&model);
        let rec = find(&pdf).unwrap();
        assert!(!rec.pending.flags.removes);
        let mut recovered = restored(&pdf, &rec, None);
        let planned = plan(&recovered.recovery_input(), &recovered.save_target(&pdf));
        assert_eq!(planned.strategy, Strategy::Append { onto: Onto::Saved });
        assert_eq!(planned.asks, []);
        let report = save(&mut recovered, &pdf, &AskAnswers::default(), &NoProgress).unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Append));
        let before = std::fs::read(&pdf).unwrap();
        assert!(before.starts_with(&bytes) && fits(&before));

        // The comment deleted, and the sidecar of that restored.
        let page = recovered.page(0).unwrap();
        let id = recovered.state().marks_on(page).next().unwrap().id;
        recovered.commit_ops(vec![delete(id)], "delete").unwrap();
        sidecar_of(&recovered);
        let rec = find(&pdf).unwrap();
        assert!(rec.pending.flags.removes);
        let mut recovered = restored(&pdf, &rec, None);
        let target = recovered.save_target(&pdf);
        let planned = plan(&recovered.recovery_input(), &target);
        let why = Why::RecoveredRemovals;
        assert_eq!(
            planned.strategy,
            Strategy::Rewrite {
                why: vec![why.clone()]
            }
        );
        let stays = Removed {
            kind: RemovedKind::Recovered,
            count: 1,
        };
        let asks = Ask::Signatures {
            impact: vec![crate::forms::sigs::SigImpact {
                signature: "sig".into(),
                verdict: crate::forms::sigs::Verdict::Invalid,
            }],
            append_anyway: Some(AppendAnyway {
                stays_readable: vec![stays],
            }),
            reason: Some(why),
        };
        assert_eq!(planned.asks, std::slice::from_ref(&asks));
        let stop = save(&mut recovered, &pdf, &AskAnswers::default(), &NoProgress);
        assert_eq!(stop, Err(SaveStop::Ask(asks)));
        assert_eq!(std::fs::read(&pdf).unwrap(), before);
        let anyway = AskAnswers {
            append_anyway: true,
            stays_readable: true,
            ..Default::default()
        };
        let report = save(&mut recovered, &pdf, &anyway, &NoProgress).unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Append));
        let now = std::fs::read(&pdf).unwrap();
        assert!(now.starts_with(&before) && fits(&now));
        assert!(!recovered.is_dirty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Save As of a restored signed file copies the file and appends the
    /// sidecar's sections after the copy, then its own: the signature
    /// still fits its bytes, and the file it was is as it was.
    #[test]
    fn a_restored_signed_file_saved_as_another_copies_and_appends() {
        let dir = scratch();
        let mut f = Form::new(1);
        f.text("t", 0, row(0));
        let sig = f.signature("sig", 0, row(1));
        f.sign(sig);
        let bytes = f.bytes();
        let pdf = dir.join("signed.pdf");
        std::fs::write(&pdf, &bytes).unwrap();
        let mut model = crate::open::file(&pdf, None).unwrap().doc;
        crate::serialize::tests::mark(&mut model);
        sidecar_of(&model);
        let rec = find(&pdf).unwrap();
        let mut recovered = restored(&pdf, &rec, None);
        let copy = dir.join("copy.pdf");
        let planned = plan(&recovered.recovery_input(), &recovered.save_target(&copy));
        assert_eq!(planned.strategy, Strategy::Append { onto: Onto::CopyOf });
        let report = save(&mut recovered, &copy, &AskAnswers::default(), &NoProgress).unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Append));
        assert_eq!(std::fs::read(&pdf).unwrap(), bytes);
        let copied = std::fs::read(&copy).unwrap();
        assert_eq!(copied, with_sections(&pdf, &rec));
        let findings = crate::forms::check(&copied, None).unwrap().findings;
        let stale =
            |f: &crate::forms::Finding| matches!(f, crate::forms::Finding::StaleSignature { .. });
        assert!(!findings.iter().any(stale), "{findings:?}");
        assert_eq!(recovered.file().unwrap().0.as_ref(), copy.as_path());
        assert!(!recovered.is_dirty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
