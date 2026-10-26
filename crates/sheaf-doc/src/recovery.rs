//! Crash recovery (M10.5 §11.10). A document's unsaved state is kept in a
//! sidecar next to its file (`<name>.pdf.sheaf-recovery`), written a few
//! seconds after edits stop and whenever a save leaves edits after it. A
//! document no file holds yet (an untitled one, M5 proposal 2) keeps its
//! sidecar in a folder of its own instead, the app's state folder's
//! `recovery/`, named for the document ([`untitled_path`]) and claimed by
//! the one view of it ([`Claim`]).
//!
//! The sidecar is format 2:
//!
//! ```text
//! sheaf-recovery 2\n<append|full>\n<base len>\n<base stamp hex>\n<flags>\n<bytes>
//! sheaf-recovery 2\nuntitled\n<folder>\n<bytes>
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
//!   nothing of it (a Full sidecar's restore), it can't take an update, or
//!   the objects a section refers to aren't all in it.
//! - **Flags** say what an Append sidecar's sections do, for the save that
//!   writes them: `removes`, when they remove or replace text the base has
//!   (by the path's effects, a transform that would need a Rewrite, or the
//!   removal check's find); `fill`, `annot` and `other`, their classes of
//!   change for the base's signatures; and `field:` with a field's full
//!   name in UTF-8 hex, for each field they touch. A Full sidecar's first
//!   save is a Rewrite, and it has none.
//! - **Untitled** is the whole document, as Full is, with no base: no file
//!   holds it. Its head says the folder its Save As starts in, the folder
//!   of what it was made from, as UTF-8 hex (empty when it has none, or
//!   its path isn't UTF-8). [`find`] beside a file takes none, and
//!   [`find_untitled`] takes nothing else.
//!
//! An untitled sidecar is its document's by name, and nothing but the
//! name ties the two together, so each name is claimed ([`Claim`]): by a
//! lock on `.<name>.sheaf-lock` beside it, which the one view of the
//! document holds for its life (and each write of its sidecar in flight
//! for the write's), in whichever sheaf process it's in. A name another
//! holds is no new document's, and a sidecar another holds is no crash's;
//! the system lets go of a dead process's locks.
//!
//! A restored document opens from memory
//! ([`MemoryKind::Recovered`](crate::store::MemoryKind::Recovered)): the
//! base's bytes with the sections after them, read as one, or the whole
//! document. Its record is the file's with the sidecar pending
//! ([`SaveRecord::restored`]), so it isn't what the file holds until a
//! save writes the sections: its first save appends them, then its own,
//! unless they removed text (`Why::RecoveredRemovals`). An untitled
//! document's opens the same way, with no record ([`restore_untitled`]);
//! once it's ready, [`never_saved`](crate::Document::never_saved) leaves
//! it with no saved version, so it's dirty, and its first save is a
//! Rewrite to wherever the reader saves it.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, BufRead as _, BufReader, Read as _};
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

/// The kind of an untitled document's sidecar, which has no base.
const UNTITLED: &str = "untitled";

/// The longest head an untitled sidecar has: its kind, and its folder's
/// path in hex. A longer one isn't this format.
const HEAD_MAX: u64 = 64 * 1024;

/// The file in the untitled sidecars' folder that's locked a moment
/// whenever a name's lock file is opened or removed there, so none is
/// removed while another has it open.
const GUARD: &str = ".lock";

/// What a claimed name's lock file, `.<name>` and this, ends in.
const LOCK: &str = ".sheaf-lock";

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
        None => ("full", String::new(), whole(input, target, p)?),
    };
    let head = format!("{kind}\n{}\n{stamp}\n{flags}\n", base.len);
    durable::write_parts(&sidecar_path(file), &[FORMAT, head.as_bytes(), &body], None)?;
    Ok(FORMAT.len() + head.len() + body.len())
}

/// The sidecar of the untitled document `name` in `dir`, the folder that
/// keeps them: `<dir>/<name>.sheaf-recovery`.
pub fn untitled_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}{SUFFIX}"))
}

/// Write `input`, the state of a document no file holds
/// ([`recovery_input`]), to the sidecar `claim` holds as the untitled
/// kind: `folder`, where its Save As starts, and the whole document, as a
/// Rewrite writes it. Its folder is made when it isn't there. Returns its
/// size.
///
/// [`recovery_input`]: crate::Document::recovery_input
pub fn write_untitled(
    input: &SaveInput,
    claim: &Claim,
    folder: &Path,
    p: &dyn Progress,
) -> Result<usize> {
    let sidecar = claim.sidecar();
    // No file is the document's: a Rewrite reads nothing at the path, and
    // names it only in its output's record, which a sidecar drops.
    let target = SaveTarget {
        path: sidecar.as_path().into(),
        same_file: false,
        profile: Profile::default(),
        stamp: None,
    };
    let body = whole(input, target, p)?;
    let folder = folder.to_str().map(hex).unwrap_or_default();
    let head = format!("{UNTITLED}\n{folder}\n");
    std::fs::create_dir_all(&claim.dir)
        .with_context(|| format!("making {}", claim.dir.display()))?;
    durable::write_parts(&sidecar, &[FORMAT, head.as_bytes(), &body], None)?;
    Ok(FORMAT.len() + head.len() + body.len())
}

/// The whole of `input`'s current version, written as a save to `target`
/// writes it: a Rewrite's plan, for its transforms, though a sidecar asks
/// nothing and is never the saved file.
fn whole(input: &SaveInput, target: SaveTarget, p: &dyn Progress) -> Result<Vec<u8>> {
    let plan = materialize::forced_rewrite(input, &target);
    Ok(materialize::materialize(input, &plan, target, p)?.bytes)
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
        words.push(format!("{FIELD}{}", hex(name)));
    }
    words.join(" ")
}

/// `text`'s UTF-8 in hex, as a sidecar's head says a name or a path.
fn hex(text: &str) -> String {
    text.bytes().map(|b| format!("{b:02x}")).collect()
}

/// The text [`hex`] says as `hex`, when that's all it says.
fn unhex(hex: &str) -> Option<String> {
    if !hex.len().is_multiple_of(2) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok()
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
            flags.fields.insert(unhex(word.strip_prefix(FIELD)?)?);
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
/// it's this format, a file's, and says nothing it doesn't know.
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
        // An untitled document's has no base, and is no file's.
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

/// The untitled document `name`'s sidecar in `dir`, claimed by one view
/// of it in one process ([`claim_untitled`]): its lock file's lock, held
/// for the claim's life. Its sidecar is written and removed only by the
/// claim's holder, so another document never takes its name, nor the next
/// launch its sidecar, while it's held. Let go, its lock file goes.
#[derive(Debug)]
pub struct Claim {
    dir: PathBuf,
    name: String,
    /// The lock file, locked; taken as the claim is let go.
    lock: Option<File>,
}

impl Claim {
    /// The document's name, its sidecar's without [`SUFFIX`].
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The sidecar it claims ([`untitled_path`]).
    pub fn sidecar(&self) -> PathBuf {
        untitled_path(&self.dir, &self.name)
    }
}

impl Drop for Claim {
    /// The lock file is removed while it's still locked and the guard is
    /// held, so no one has it open meanwhile: one opened later is a new
    /// file, and no two claims ever lock two files of one name. Without
    /// the guard, the file is left for the next claim.
    fn drop(&mut self) {
        let guard = guard(&self.dir);
        if guard.is_ok() {
            let _ = std::fs::remove_file(lock_path(&self.dir, &self.name));
        }
        drop(self.lock.take());
    }
}

/// The lock file of the untitled document `name` in `dir`.
fn lock_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!(".{name}{LOCK}"))
}

/// `dir`'s guard ([`GUARD`]), locked until it's dropped.
fn guard(dir: &Path) -> io::Result<File> {
    let guard = (OpenOptions::new().read(true).write(true))
        .create(true)
        .truncate(false)
        .open(dir.join(GUARD))?;
    guard.lock()?;
    Ok(guard)
}

/// Claim the untitled document `name`'s sidecar in `dir` (made when it
/// isn't there): None while a view holds it, in this process or another.
/// Whether a sidecar is there already is the caller's to see, once it
/// holds the claim.
pub fn claim_untitled(dir: &Path, name: &str) -> io::Result<Option<Claim>> {
    std::fs::create_dir_all(dir)?;
    let _guard = guard(dir)?;
    let lock = (OpenOptions::new().read(true).write(true))
        .create(true)
        .truncate(false)
        .open(lock_path(dir, name))?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(Claim {
            dir: dir.to_path_buf(),
            name: name.to_string(),
            lock: Some(lock),
        })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// An untitled document's recovery data, found in the folder that keeps
/// them, claimed while it's asked about: its body is read only once it's
/// restored.
#[derive(Debug)]
pub struct Untitled {
    pub claim: Claim,
    /// Where its Save As starts, as its head says: empty when it says
    /// none.
    pub folder: PathBuf,
}

/// The untitled documents' sidecars in `dir` that no view holds, a
/// crash's, by name and claimed; none when there's no such folder. Each
/// is read only as far as its head. One that isn't this format's
/// untitled kind is removed, and so are the temp files writes of an
/// unheld name left and its lock file; a sidecar that can't be read now
/// and anything else are left.
pub fn find_untitled(dir: &Path) -> Vec<Untitled> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    // Each name's files: whether it has a sidecar, and its temp files.
    let mut names: BTreeMap<String, (bool, Vec<PathBuf>)> = BTreeMap::new();
    for entry in entries.flatten() {
        let Some(file) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if let Some(name) = file.strip_suffix(SUFFIX) {
            names.entry(name.into()).or_default().0 = true;
        } else if let Some(name) = temp_of(&file) {
            names.entry(name.into()).or_default().1.push(entry.path());
        } else if let Some(name) = file.strip_prefix('.').and_then(|f| f.strip_suffix(LOCK)) {
            names.entry(name.into()).or_default();
        }
    }
    (names.into_iter())
        .filter_map(|(name, (sidecar, temps))| {
            // Held: a live document's, whose writes these are.
            let claim = claim_untitled(dir, &name).ok().flatten()?;
            for temp in temps {
                let _ = std::fs::remove_file(temp);
            }
            if !sidecar {
                return None;
            }
            match untitled_folder(&claim.sidecar()) {
                Ok(Some(folder)) => Some(Untitled { claim, folder }),
                Ok(None) => {
                    let _ = std::fs::remove_file(claim.sidecar());
                    None
                }
                Err(_) => None,
            }
        })
        .collect()
}

/// The untitled document whose sidecar's temp file `file` is
/// (`.<name>.sheaf-recovery.<random>.sheaf-tmp`), when its name is whole.
fn temp_of(file: &str) -> Option<&str> {
    let (sidecar, random) = (file.strip_prefix('.')?)
        .strip_suffix(".sheaf-tmp")?
        .rsplit_once('.')?;
    let random = random.len() == 16 && random.bytes().all(|b| b.is_ascii_hexdigit());
    random.then(|| sidecar.strip_suffix(SUFFIX)).flatten()
}

/// The folder the untitled sidecar at `sidecar` says, read from its head
/// alone, when it's this format's untitled kind and holds a document.
fn untitled_folder(sidecar: &Path) -> io::Result<Option<PathBuf>> {
    let file = File::open(sidecar)?;
    let len = file.metadata()?.len();
    let head = untitled_head(&mut BufReader::new(file))?;
    Ok(head.filter(|&(_, at)| at < len).map(|(folder, _)| folder))
}

/// The folder an untitled sidecar's head says, and where its document
/// starts, when it's this format's untitled kind: read from `reader`,
/// which is left there.
fn untitled_head(reader: &mut impl io::BufRead) -> io::Result<Option<(PathBuf, u64)>> {
    let mut head = reader.take(HEAD_MAX);
    let mut lines = Vec::with_capacity(3);
    let mut at = 0;
    for _ in 0..3 {
        let mut line = Vec::new();
        let n = head.read_until(b'\n', &mut line)?;
        if line.pop() != Some(b'\n') {
            return Ok(None);
        }
        at += n as u64;
        lines.push(line);
    }
    let kind = (lines[0].as_slice(), lines[1].as_slice());
    if kind != (&FORMAT[..FORMAT.len() - 1], UNTITLED.as_bytes()) {
        return Ok(None);
    }
    let folder = std::str::from_utf8(&lines[2]).ok().and_then(unhex);
    Ok(folder.map(|folder| (PathBuf::from(folder), at)))
}

/// The document the untitled sidecar `claim` holds, opened from memory
/// into `store`: its body read whole, once. It has no record: once it's
/// ready, it's [`never_saved`], so it's dirty and its first save is a
/// Rewrite.
///
/// [`never_saved`]: crate::Document::never_saved
pub fn restore_untitled(store: &Store, claim: &Claim) -> Result<Src> {
    let sidecar = claim.sidecar();
    let reading = || format!("reading {}", sidecar.display());
    let file = File::open(&sidecar).with_context(reading)?;
    let len = file.metadata().with_context(reading)?.len();
    let mut file = BufReader::new(file);
    let at = untitled_head(&mut file)
        .with_context(reading)?
        .map(|(_, at)| at);
    let Some(body) = at.and_then(|at| len.checked_sub(at)).filter(|&n| n > 0) else {
        bail!(
            "{} isn't an untitled document's recovery data",
            sidecar.display()
        );
    };
    let body = usize::try_from(body).context("recovery data too large")?;
    let bytes = sheaf_util::read_shared(&mut file, body).with_context(reading)?;
    (store.open(bytes, BlobKind::recovered(None)))
        .with_context(|| format!("opening the recovery data of {}", claim.name))
}

/// Remove the untitled document's sidecar `claim` holds, if it's there.
pub fn discard_untitled(claim: &Claim) {
    let _ = std::fs::remove_file(claim.sidecar());
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
        AppendAnyway, Ask, AskAnswers, Onto, SaveHow, SaveStop, Strategy, StrategyKind, Why, plan,
        save_with,
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
            BlobKind::Image(_) | BlobKind::Attachment => unreachable!("a PDF"),
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
        assert_eq!(contents.len(), 4, "isolated, then the block");
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
        let report = save_with(
            &mut recovered,
            &pdf,
            SaveHow::Planned,
            &AskAnswers::default(),
            &NoProgress,
        )
        .unwrap();
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

    /// An unsaved header and footer is sections after the file, as any
    /// edit is: encrypted as the file is, the header's text nowhere in them
    /// in the clear. They restore with the header, and the first save
    /// appends them, then its own, after the file.
    #[test]
    fn an_unsaved_header_s_sidecar_appends() {
        use crate::serialize::fixture::{USER, encrypted, form};

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
        assert_eq!(kind_of(&pdf), "append");
        let sidecar = std::fs::read(sidecar_path(&pdf)).unwrap();
        assert!(!holds(&sidecar, b"HEADERSECRET"));
        let rec = find(&pdf).unwrap();
        assert!(!rec.pending.flags.removes);
        let mut recovered = restored(&pdf, &rec, Some(USER));
        let texts = |doc: &Document| {
            let hf = doc.state().header_footer().expect("a header and footer");
            hf.spec.header_footer().unwrap().texts[0].clone()
        };
        assert_eq!(texts(&recovered), "HEADERSECRET <<1>>");
        let planned = plan(&recovered.recovery_input(), &recovered.save_target(&pdf));
        assert_eq!(planned.strategy, Strategy::Append { onto: Onto::Saved });
        let report = save_with(
            &mut recovered,
            &pdf,
            SaveHow::Planned,
            &AskAnswers::default(),
            &NoProgress,
        )
        .unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Append));
        let written = std::fs::read(&pdf).unwrap();
        assert!(written.starts_with(&table));
        assert!(written[table.len()..].starts_with(&rec.bytes));
        let reopened = crate::open::file(&pdf, Some(USER)).unwrap().doc;
        assert_eq!(texts(&reopened), "HEADERSECRET <<1>>");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A Full sidecar of an encrypted file, written when no section can
    /// follow the file (here its record says it can't take an update), is
    /// the whole document encrypted as the file is, packed when the file's
    /// cross-references are a stream: neither a header's text nor the
    /// form's value is in it in the clear, even inflated. It restores with
    /// a record that says nothing of the file, so its first save is a
    /// Rewrite, and its own sidecar is Full too, against the same file.
    #[test]
    fn an_encrypted_file_s_full_sidecar_holds_no_plaintext() {
        use crate::serialize::fixture::{
            USER, VALUE, encrypted, form, in_clear, object_streams_of,
        };

        let dir = scratch();
        let plain = form();
        let [(_, state), ..] = crate::serialize::fixture::rc4_aes_128_and_aes_256(&plain);
        let [table, stream] = encrypted(&plain, &state, |_| {});
        for (kind, file) in [("table", table), ("stream", stream)] {
            let pdf = dir.join(format!("{kind}.pdf"));
            std::fs::write(&pdf, &file).unwrap();
            let mut model = crate::open::file(&pdf, Some(USER)).unwrap().doc;
            let set = fx::set_header(fx::header("HEADERSECRET <<1>>"), None);
            model.commit_ops(vec![set], "header").unwrap();
            let mut input = model.recovery_input();
            let saved = input.saved.as_mut().unwrap();
            let record = SaveRecord {
                sound: false,
                ..SaveRecord::clone(&saved.record)
            };
            saved.record = Arc::new(record);
            let (file, base) = model.file().map(|(f, s)| (f.clone(), *s)).unwrap();
            write(&input, &file, &base, &NoProgress).unwrap();
            assert_eq!(kind_of(&pdf), "full", "{kind}");
            let sidecar = std::fs::read(sidecar_path(&pdf)).unwrap();
            for text in [b"HEADERSECRET".as_slice(), VALUE.as_bytes()] {
                assert!(!in_clear(&sidecar, text), "{kind}");
            }
            let rec = find(&pdf).unwrap();
            assert!(rec.pending.bytes.is_none(), "{kind}");
            let whole = LazyPdf::open(rec.bytes.clone(), Some(USER), &Limits::DEFAULT.reader);
            let packed = !object_streams_of(&whole.unwrap()).is_empty();
            assert_eq!(packed, kind == "stream", "{kind}");
            let recovered = restored(&pdf, &rec, Some(USER));
            assert!(recovered.is_dirty(), "{kind}");
            assert!(!recovered.saved_record().unwrap().knows_file(), "{kind}");
            let hf = recovered.state().header_footer().unwrap();
            assert_eq!(
                hf.spec.header_footer().unwrap().texts[0],
                "HEADERSECRET <<1>>",
                "{kind}"
            );
            let planned = plan(&recovered.recovery_input(), &recovered.save_target(&pdf));
            let why = vec![Why::NoSaveRecord];
            assert_eq!(planned.strategy, Strategy::Rewrite { why }, "{kind}");
            sidecar_of(&recovered);
            assert_eq!(kind_of(&pdf), "full", "{kind}");
            let again = restored(&pdf, &find(&pdf).unwrap(), Some(USER));
            let hf = again.state().header_footer().unwrap();
            assert_eq!(
                hf.spec.header_footer().unwrap().texts[0],
                "HEADERSECRET <<1>>",
                "{kind}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A header that numbers the pages, saved, and the pages reordered: the
    /// sidecar's section draws the numbers on the pages that moved again,
    /// their old blocks let go as a live redraw, so it removes nothing.
    /// Restored, the first save appends it, and the file reopens with each
    /// page numbered where it is now.
    #[test]
    fn a_live_redraw_s_sidecar_appends() {
        let dir = scratch();
        let pdf = dir.join("doc.pdf");
        two_page_pdf(&pdf);
        let mut model = crate::open::file(&pdf, None).unwrap().doc;
        turn(&mut model, 0);
        let set = fx::set_header(fx::header("Page <<1>> of <<N>>"), None);
        model.commit_ops(vec![set], "header").unwrap();
        let report = save_with(
            &mut model,
            &pdf,
            SaveHow::Planned,
            &AskAnswers::default(),
            &NoProgress,
        )
        .unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Append));
        let [first, second] = [0, 1].map(|i| model.page(i).unwrap());
        let reorder = fx::move_pages(&[second], Some(first));
        model.commit_ops(vec![reorder], "reorder").unwrap();
        sidecar_of(&model);
        assert_eq!(kind_of(&pdf), "append");
        let rec = find(&pdf).unwrap();
        assert!(!rec.pending.flags.removes);
        let mut recovered = restored(&pdf, &rec, None);
        let report = save_with(
            &mut recovered,
            &pdf,
            SaveHow::Planned,
            &AskAnswers::default(),
            &NoProgress,
        )
        .unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Append));
        assert_eq!(report.why, []);
        let reopened = crate::open::file(&pdf, None).unwrap().doc;
        let turned = reopened.page(1).unwrap();
        assert_eq!(reopened.geom(turned).rotate, Rotation::R90);
        let file = Pdf::load(&pdf).unwrap();
        for (n, &page) in (1..).zip(file.get_pages().values()) {
            let text = String::from_utf8_lossy(&file.get_page_content(page)).into_owned();
            assert!(text.contains(&format!("(Page {n} of 2) Tj")), "{text}");
        }
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
        let report = save_with(
            &mut model,
            &pdf,
            SaveHow::Planned,
            &AskAnswers::default(),
            &NoProgress,
        )
        .unwrap();
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
        let report = save_with(
            &mut third,
            &pdf,
            SaveHow::Planned,
            &AskAnswers::default(),
            &NoProgress,
        )
        .unwrap();
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
        let report = save_with(
            &mut recovered,
            &pdf,
            SaveHow::Planned,
            &AskAnswers::default(),
            &NoProgress,
        )
        .unwrap();
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
        let stop = save_with(
            &mut recovered,
            &pdf,
            SaveHow::Planned,
            &AskAnswers::default(),
            &NoProgress,
        );
        assert_eq!(stop, Err(SaveStop::Ask(asks)));
        assert_eq!(std::fs::read(&pdf).unwrap(), before);
        let anyway = AskAnswers {
            append_anyway: true,
            stays_readable: true,
            ..Default::default()
        };
        let report =
            save_with(&mut recovered, &pdf, SaveHow::Planned, &anyway, &NoProgress).unwrap();
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
        let report = save_with(
            &mut recovered,
            &copy,
            SaveHow::Planned,
            &AskAnswers::default(),
            &NoProgress,
        )
        .unwrap();
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

    /// A document made in memory, as an untitled one starts: blank pages
    /// `widths` points wide and Letter's height, the second given the
    /// recognized word "Hello", as `sidecar_carries_ocr_layers` gives it.
    fn untitled(widths: &[f32]) -> Document {
        use crate::annot::Box2;
        use crate::text_layer::OcrWord;
        use sheaf_edit::{Bring, PageSize, PageSource};
        let store = Store::new();
        let empty = crate::memory::empty(&store).unwrap();
        let policy = crate::history::default_policy();
        let mut doc = crate::open::start(store, empty.clone(), "Untitled".into(), None, policy);
        doc.ready(crate::open::finish(&empty, &NoProgress)).unwrap();
        let blank = |&width: &f32| Op::InsertPages {
            from: PageSource::Blank {
                size: PageSize {
                    width,
                    height: 792.0,
                },
                count: 1,
            },
            pages: None,
            before: None,
            bring: Bring::default(),
            locals: Vec::new(),
        };
        doc.commit_ops(widths.iter().map(blank).collect(), "blank")
            .unwrap();
        let p1 = doc.page(1).unwrap();
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
        doc.commit_ops(fx::ocr([(p1, Arc::new(layer))]), "ocr")
            .unwrap();
        doc
    }

    /// The text page `index` (from 0) of `bytes` draws, its content's
    /// streams as one.
    fn page_text(bytes: &[u8], index: u32) -> String {
        let pdf = Pdf::load_mem(bytes).unwrap();
        let page = pdf.get_pages()[&(index + 1)];
        String::from_utf8_lossy(&pdf.get_page_content(page)).into_owned()
    }

    /// What follows the head of the untitled sidecar at `sidecar`: its
    /// document.
    fn untitled_body(sidecar: &Path) -> Vec<u8> {
        let bytes = std::fs::read(sidecar).unwrap();
        let mut reader = bytes.as_slice();
        let (_, at) = untitled_head(&mut reader).unwrap().unwrap();
        bytes[at as usize..].to_vec()
    }

    /// The temp files in `dir`.
    fn temps(dir: &Path) -> Vec<PathBuf> {
        (std::fs::read_dir(dir).unwrap())
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().ends_with(".sheaf-tmp"))
            .collect()
    }

    /// An untitled document's sidecar is the whole document, in the folder
    /// that keeps them, named for it, with the folder its Save As starts
    /// in: found there by that name once its claim is let go, it restores
    /// from memory with no record, its pages the sizes they were, the
    /// recognized word where it was. Never saved, it's dirty with nothing
    /// to undo, pruned too, and its first save is a Rewrite that leaves it
    /// clean, the file its own from then on.
    #[test]
    fn an_untitled_sidecar_round_trips() {
        let dir = scratch();
        let doc = untitled(&[612.0, 595.0, 612.0]);
        let folder = dir.join("recovery");
        let claim = claim_untitled(&folder, "Untitled scan 1").unwrap().unwrap();
        let sidecar = untitled_path(&folder, "Untitled scan 1");
        assert_eq!(claim.sidecar(), sidecar);
        let images = dir.join("images ü");
        let n = write_untitled(&doc.recovery_input(), &claim, &images, &NoProgress).unwrap();
        assert_eq!(std::fs::metadata(&sidecar).unwrap().len() as usize, n);
        assert!(temps(&folder).is_empty(), "nothing of it in a temp file");
        assert!(
            find_untitled(&folder).is_empty(),
            "claimed, it's no crash's"
        );
        drop(claim);
        let found = find_untitled(&folder);
        let [rec] = found.as_slice() else {
            panic!("one sidecar: {found:?}")
        };
        assert_eq!(rec.claim.name(), "Untitled scan 1");
        assert_eq!(rec.claim.sidecar(), sidecar);
        assert_eq!(rec.folder, images);

        let store = Store::new();
        let src = restore_untitled(&store, &rec.claim).unwrap();
        let body = untitled_body(&sidecar);
        assert!(body.starts_with(b"%PDF-"));
        assert_eq!(**src.blob().bytes(), *body);
        let origin = match src.blob().kind() {
            BlobKind::Pdf { origin, .. } => origin.clone(),
            BlobKind::Image(_) | BlobKind::Attachment => unreachable!("a PDF"),
        };
        assert_eq!(origin, Origin::Memory(MemoryKind::Recovered));
        let policy = crate::history::default_policy();
        let name = rec.claim.name().into();
        let mut back = crate::open::start(store, src.clone(), name, None, policy);
        back.ready(crate::open::finish(&src, &NoProgress)).unwrap();
        back.never_saved();
        let sizes = |d: &Document| {
            (0..d.page_count())
                .map(|i| d.page_size(d.page(i).unwrap()))
                .collect::<Vec<_>>()
        };
        assert_eq!(sizes(&back), sizes(&doc));
        let hello = "<00480065006C006C006F>";
        let texts: Vec<String> = (0..3).map(|i| page_text(&body, i)).collect();
        assert!(
            texts[1].contains("3 Tr") && texts[1].contains(hello),
            "{texts:?}"
        );
        assert!(!texts[0].contains(hello) && !texts[2].contains(hello));
        assert_eq!(**back.name(), *"Untitled scan 1");
        assert!(back.is_dirty() && !back.can_undo());
        assert!(back.saved_record().is_none() && back.file().is_none());

        // Pruned to its last edit, it's no less dirty.
        turn(&mut back, 0);
        assert!(back.forget_before_cursor());
        assert!(back.is_dirty() && !back.can_undo());
        let path = dir.join("saved.pdf");
        let none = AskAnswers::default();
        let report = save_with(&mut back, &path, SaveHow::Planned, &none, &NoProgress).unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
        assert!(!back.is_dirty());
        assert_eq!(back.file().unwrap().0.as_ref(), path.as_path());
        let saved = std::fs::read(&path).unwrap();
        assert_eq!(Pdf::load_mem(&saved).unwrap().get_pages().len(), 3);
        assert!(page_text(&saved, 1).contains(hello));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A name is one claim's at a time: another claim of it is refused
    /// while it's held, by this process too (the lock is the open file's),
    /// and its sidecar and its writes' temp files aren't a crash's then.
    /// Let go, its lock file goes, and the name is free again. A sidecar
    /// with no folder says so.
    #[test]
    fn a_claimed_name_is_no_one_else_s() {
        let dir = scratch();
        let held = claim_untitled(&dir, "Untitled scan 1").unwrap().unwrap();
        let lock = lock_path(&dir, "Untitled scan 1");
        assert!(lock.exists());
        assert!(claim_untitled(&dir, "Untitled scan 1").unwrap().is_none());
        let input = untitled(&[612.0, 612.0]).recovery_input();
        write_untitled(&input, &held, Path::new(""), &NoProgress).unwrap();
        let temp = dir.join(".Untitled scan 1.sheaf-recovery.0123456789abcdef.sheaf-tmp");
        std::fs::write(&temp, "%PDF-").unwrap();
        assert!(find_untitled(&dir).is_empty());
        assert!(held.sidecar().exists() && temp.exists());
        let other = claim_untitled(&dir, "Untitled scan 2").unwrap().unwrap();
        drop(held);
        assert!(!lock.exists());
        assert!(lock_path(&dir, "Untitled scan 2").exists());
        let found = find_untitled(&dir);
        let [rec] = found.as_slice() else {
            panic!("one sidecar: {found:?}")
        };
        assert_eq!(rec.claim.name(), "Untitled scan 1");
        assert_eq!(rec.folder, PathBuf::new());
        assert!(!temp.exists(), "a dead write's temp file goes");
        assert!(claim_untitled(&dir, "Untitled scan 1").unwrap().is_none());
        drop(found);
        assert!(claim_untitled(&dir, "Untitled scan 1").unwrap().is_some());
        drop(other);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The folder's sidecars are found by name, from their heads alone: one
    /// whose document isn't a PDF is found, and fails as it's restored.
    /// What isn't an untitled sidecar of this format goes: one with nothing
    /// after its head, or a head cut short, one of the format before, a
    /// file's, one of a kind it doesn't know, one whose folder isn't hex.
    /// The temp file and the lock file of a name no one holds go too. One
    /// it can't read is left, and so are a temp file whose name was cut
    /// short and anything not named as a sidecar's. A folder that isn't
    /// there has none.
    #[test]
    fn find_untitled_removes_what_it_can_t_read() {
        let dir = scratch();
        assert!(find_untitled(&dir.join("recovery")).is_empty());
        let input = untitled(&[612.0, 612.0]).recovery_input();
        for name in ["Untitled scan 2", "Untitled scan 1"] {
            let claim = claim_untitled(&dir, name).unwrap().unwrap();
            write_untitled(&input, &claim, &dir, &NoProgress).unwrap();
        }
        let bad = [
            ("Empty", "sheaf-recovery 2\nuntitled\n\n"),
            ("Short", "sheaf-recovery 2\nuntitled\n"),
            ("Old", "sheaf-recovery 1\n12\n%PDF-"),
            ("A file's", "sheaf-recovery 2\nfull\n5\n00\n\n%PDF-"),
            ("Another", "sheaf-recovery 2\nuntitled2\n\n%PDF-"),
            ("Not hex", "sheaf-recovery 2\nuntitled\n2f7\n%PDF-"),
        ];
        for (name, bytes) in bad {
            std::fs::write(untitled_path(&dir, name), bytes).unwrap();
        }
        let garbled = untitled_path(&dir, "Garbled");
        std::fs::write(&garbled, "sheaf-recovery 2\nuntitled\n\nnot a PDF").unwrap();
        let gone = [
            dir.join(".Untitled scan 3.sheaf-recovery.0123456789abcdef.sheaf-tmp"),
            lock_path(&dir, "Untitled scan 4"),
        ];
        let left = [
            dir.join("notes.txt"),
            dir.join(".Untitled scan 3.sheaf-recov.0123456789abcdef.sheaf-tmp"),
        ];
        for path in gone.iter().chain(&left) {
            std::fs::write(path, "%PDF-").unwrap();
        }
        let unreadable = untitled_path(&dir, "A folder");
        std::fs::create_dir(&unreadable).unwrap();

        let found = find_untitled(&dir);
        let names: Vec<&str> = found.iter().map(|u| u.claim.name()).collect();
        assert_eq!(names, ["Garbled", "Untitled scan 1", "Untitled scan 2"]);
        assert!(found.iter().skip(1).all(|u| u.folder == dir));
        for (name, _) in bad {
            assert!(!untitled_path(&dir, name).exists(), "{name}");
        }
        assert!(gone.iter().all(|path| !path.exists()), "{gone:?}");
        assert!(left.iter().all(|path| path.exists()));
        assert!(unreadable.is_dir());
        let store = Store::new();
        assert!(restore_untitled(&store, &found[0].claim).is_err());
        assert!(garbled.exists(), "left for the caller to drop");
        discard_untitled(&found[1].claim);
        discard_untitled(&found[1].claim); // no-op when gone
        drop(found);
        let found = find_untitled(&dir);
        let names: Vec<&str> = found.iter().map(|u| u.claim.name()).collect();
        assert_eq!(names, ["Garbled", "Untitled scan 2"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An untitled sidecar beside a file is no recovery of the file's:
    /// `find` removes it as malformed, even one whose bytes after its head
    /// read as the file's base.
    #[test]
    fn find_beside_a_file_takes_no_untitled_sidecar() {
        let dir = scratch();
        let pdf = dir.join("doc.pdf");
        two_page_pdf(&pdf);
        let mut model = crate::open::file(&pdf, None).unwrap().doc;
        turn(&mut model, 1);
        let claim = claim_untitled(&dir, "doc.pdf").unwrap().unwrap();
        assert_eq!(claim.sidecar(), sidecar_path(&pdf));
        write_untitled(&model.recovery_input(), &claim, &dir, &NoProgress).unwrap();
        assert_eq!(kind_of(&pdf), UNTITLED);
        assert!(find(&pdf).is_none());
        assert!(!sidecar_path(&pdf).exists());
        let base = FileStamp::read(&pdf).unwrap();
        let stamp = base.hex().unwrap();
        let like_a_base = format!("sheaf-recovery 2\nuntitled\n{}\n{stamp}\n\n%PDF-", base.len);
        std::fs::write(sidecar_path(&pdf), like_a_base).unwrap();
        assert!(find(&pdf).is_none());
        assert!(!sidecar_path(&pdf).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
