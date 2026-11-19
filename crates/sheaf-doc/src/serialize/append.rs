//! An update after a file's last section (M10.5 §11.7, M18 slice 12): the
//! objects it writes, each encrypted on its own when the file is, and a
//! section in the file's cross-reference kind whose runs list only those,
//! its `/Prev` at the last section's real start.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Result;

use super::crypt::Crypt;
use super::durable::{self, FileStamp, WriteError, read_range, startxref};
use super::xref::{Entry, Section};
use crate::materialize::ObjKey;
use crate::pdf::{Dictionary, Object, ObjectId, XrefKind, deflate_options, resolve_fn};
use crate::source::Source;

/// The file an update goes after, as its last section describes it.
#[derive(Debug, Clone)]
pub struct Base {
    /// Its length: where the update starts.
    pub len: usize,
    /// Whether its last line is ended; if not, the update ends it.
    pub ends_line: bool,
    /// Where its header, `%PDF-`, starts: positions count from there.
    pub header: usize,
    /// Where its last section starts, from the header.
    pub prev: usize,
    /// Past every object number it uses, its cross-reference streams'
    /// included: where the update's new numbers start.
    pub size: u32,
    /// Its last section's kind, which the update's is.
    pub kind: XrefKind,
    /// Its `/ID[0]`, read as an encrypted file's key is made from it.
    pub first_id: Option<Vec<u8>>,
    /// Its catalog.
    pub root: ObjectId,
    /// Its document information, as the trailer gives it.
    pub info: Option<ObjectId>,
}

impl Base {
    /// `src`'s bytes, as their last section describes them: what an
    /// overlay's section goes after (M10.5 §10.1).
    pub fn over(src: &Source) -> Option<Base> {
        let bytes = src.bytes();
        let xref = src.xref();
        let trailer = src.pdf().trailer();
        let root = trailer.get(b"Root").and_then(Object::as_reference)?;
        let info = trailer.get(b"Info").and_then(Object::as_reference);
        let used = src.pdf().ids().map(|id| id.number.saturating_add(1)).max();
        let size = [
            xref.size,
            xref.max_stream.map_or(0, |n| n.saturating_add(1)),
            used.unwrap_or(0),
        ];
        Some(Base {
            len: bytes.len(),
            ends_line: matches!(bytes.last(), Some(b'\n' | b'\r')),
            header: xref.header_offset,
            prev: xref.start,
            size: size.into_iter().max().unwrap_or(1).max(1),
            kind: xref.kind,
            first_id: xref.id.as_ref().map(|[first, _]| first.clone()),
            root,
            info,
        })
    }
}

/// An update as written.
#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub bytes: Vec<u8>,
    /// Where its section starts, from the header: what `startxref` says.
    pub start: usize,
    /// Its `/Size`.
    pub size: u32,
    /// Its `/ID`, when it has one.
    pub id: Option<[Vec<u8>; 2]>,
}

/// Writes an incremental update section after `base` containing `objects`.
pub fn section(
    base: &Base,
    objects: &BTreeMap<ObjectId, (ObjKey, Object)>,
    xref_number: u32,
    crypt: Option<&Crypt>,
) -> Result<Option<Update>> {
    if objects.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::new();
    if !base.ends_line {
        out.push(b'\n');
    }

    let mut trailer = Dictionary::new();
    trailer.set("Root", base.root);
    trailer.set("Prev", base.prev as i64);
    if let Some(info) = base.info {
        trailer.set("Info", info);
    }
    if let Some(crypt) = crypt {
        trailer.set("Encrypt", crypt.at());
    }

    let resolve = resolve_fn(|id| {
        objects
            .get(&id)
            .map(|(_, obj)| obj)
            .or_else(|| crypt.and_then(|c| c.fallback_resolve(id)))
    });
    let encrypting = match crypt {
        Some(c) => Some(c.laid_out(c.at(), &trailer, &resolve)?),
        None => None,
    };

    let mut section = Section::append(base.size, base.header);
    for (id, (_, obj)) in objects {
        let offset = base.len + super::indirect_as(&mut out, *id, obj, encrypting.as_ref())?;
        section.insert(
            id.number,
            Entry::InUse {
                offset,
                generation: id.generation,
            },
        );
    }

    let kept = crypt
        .and_then(Crypt::first_id)
        .map(<[u8]>::to_vec)
        .or_else(|| base.first_id.clone());
    let id = super::file_id(kept, crypt.is_some(), &out);
    if let Some(id) = &id {
        trailer.set("ID", id.clone());
    }
    let pos = base.len + out.len();
    let size = match base.kind {
        XrefKind::Table => {
            let size = section.size();
            section.table(&mut out, trailer, pos)?;
            size
        }
        XrefKind::Stream => {
            let size = section.size().max(xref_number.saturating_add(1));
            section.stream(&mut out, xref_number, trailer, pos, deflate_options())?;
            size
        }
    };
    let id = id
        .as_ref()
        .and_then(Object::as_array)
        .and_then(|ids| match ids {
            [a, b] => Some([a.as_bytes()?.to_vec(), b.as_bytes()?.to_vec()]),
            _ => None,
        });
    Ok(Some(Update {
        bytes: out,
        start: pos - base.header,
        size,
        id,
    }))
}

/// The first line of a journal in this format.
const JOURNAL: &[u8] = b"sheaf-append 1\n";
const JOURNAL_SUFFIX: &str = ".sheaf-append";

/// The journal beside `pdf` while an update is appended to it.
pub fn journal_path(pdf: &Path) -> PathBuf {
    let mut name = (pdf.file_name())
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    name.push(JOURNAL_SUFFIX);
    pdf.with_file_name(name)
}

#[derive(Debug, Clone, PartialEq)]
struct Journal {
    pre_len: u64,
    stamp: FileStamp,
    start: u64,
}

impl Journal {
    fn bytes(&self) -> io::Result<Vec<u8>> {
        let stamp = (self.stamp.hex()).ok_or_else(|| io::Error::other("a time before 1970"))?;
        let body = format!("{}\n{stamp}\n{}\n", self.pre_len, self.start);
        Ok([JOURNAL, body.as_bytes()].concat())
    }

    fn parse(bytes: &[u8]) -> Option<Journal> {
        let rest = std::str::from_utf8(bytes.strip_prefix(JOURNAL)?).ok()?;
        let mut lines = rest.strip_suffix('\n')?.split('\n');
        let pre_len = lines.next()?.parse().ok()?;
        let stamp = FileStamp::from_hex(pre_len, lines.next()?)?;
        let start = lines.next()?.parse().ok()?;
        lines.next().is_none().then_some(Journal {
            pre_len,
            stamp,
            start,
        })
    }
}

#[cfg(any(test, feature = "fixtures"))]
pub fn tear_next(after: usize) {
    TEAR.set(Some(after));
}

#[cfg(any(test, feature = "fixtures"))]
thread_local! {
    static TEAR: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

pub fn append_in_place(
    path: &Path,
    base: &FileStamp,
    section: &[u8],
) -> Result<FileStamp, WriteError> {
    let io_at = |source| WriteError::Io {
        path: path.to_path_buf(),
        source,
    };
    let changed = || WriteError::Changed {
        path: path.to_path_buf(),
    };
    let path = &durable::resolve(path).map_err(io_at)?;
    let (now, from, tail) = match FileStamp::read_keeping_tail(path) {
        Ok(read) => read,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(changed()),
        Err(e) => return Err(io_at(e)),
    };
    if now != *base {
        return Err(changed());
    }
    let start = startxref(section)
        .ok_or_else(|| io_at(io::Error::other("the update has no startxref")))?
        .0;
    let journal = Journal {
        pre_len: base.len,
        stamp: *base,
        start,
    };
    let at = journal_path(path);
    let _held = write_journal(&at, &journal).map_err(io_at)?;
    #[cfg(any(test, feature = "fixtures"))]
    if let Some(after) = TEAR.take() {
        let mut file = OpenOptions::new().append(true).open(path).map_err(io_at)?;
        file.write_all(&section[..after.min(section.len())])
            .map_err(io_at)?;
        return Err(io_at(io::Error::other("torn by a test hook")));
    }
    let done = |at: &Path| {
        if let Err(e) = remove_journal(at) {
            log::warn!("removing {}: {e}", at.display());
        }
    };
    match write_section(path, base.len, section, from, &tail) {
        Ok(Some(stamp)) => {
            done(&at);
            Ok(stamp)
        }
        Ok(None) => {
            done(&at);
            Err(changed())
        }
        Err(e) => {
            if cut_back(path, &journal).is_ok() {
                let _ = remove_journal(&at);
            }
            Err(io_at(e))
        }
    }
}

fn write_journal(at: &Path, journal: &Journal) -> io::Result<File> {
    let (tmp, mut file) = durable::temp_beside(at)?;
    let written = (|| {
        if let Err(e) = file.lock() {
            log::warn!("locking {}: {e}", tmp.display());
        }
        file.write_all(&journal.bytes()?)?;
        file.sync_all()?;
        fs::rename(&tmp, at)
    })();
    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    #[cfg(unix)]
    durable::sync_dir(durable::dir_of(at));
    Ok(file)
}

fn remove_journal(at: &Path) -> io::Result<()> {
    match fs::remove_file(at) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    #[cfg(unix)]
    durable::sync_dir(durable::dir_of(at));
    Ok(())
}

fn write_section(
    path: &Path,
    pre_len: u64,
    section: &[u8],
    from: u64,
    tail: &[u8],
) -> io::Result<Option<FileStamp>> {
    let mut file = OpenOptions::new().read(true).append(true).open(path)?;
    if file.metadata()?.len() != pre_len {
        return Ok(None);
    }
    file.write_all(section)?;
    file.sync_all()?;
    let end = [tail, section].concat();
    let len = pre_len + section.len() as u64;
    let mut stamp = FileStamp::ending(len, SystemTime::UNIX_EPOCH, from, &end, &mut |at| {
        read_range(&mut file, at)
    })?;
    drop(file);
    stamp.mtime = fs::metadata(path)?.modified()?;
    Ok(Some(stamp))
}

fn cut_back(path: &Path, journal: &Journal) -> io::Result<()> {
    let file = OpenOptions::new().write(true).open(path)?;
    file.set_len(journal.pre_len)?;
    file.set_modified(journal.stamp.mtime)?;
    file.sync_all()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Torn {
    Undone { len: u64 },
    Left,
}

impl fmt::Display for Torn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Torn::Undone { .. } => f.write_str(
                "The last save to this file didn't finish: the file is as it was before it",
            ),
            Torn::Left => f.write_str(
                "The last save to this file didn't finish, and the file changed since: \
                 it's left as it is",
            ),
        }
    }
}

pub fn undo_torn(path: &Path) -> io::Result<Option<Torn>> {
    let path = &durable::resolve(path)?;
    let at = journal_path(path);
    let mut journal = match File::open(&at) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    match journal.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => return Ok(None),
        Err(fs::TryLockError::Error(e)) => log::warn!("locking {}: {e}", at.display()),
    }
    let mut bytes = Vec::new();
    journal.read_to_end(&mut bytes)?;
    let torn = match Journal::parse(&bytes) {
        Some(journal) => torn(path, &journal)?,
        None => None,
    };
    remove_journal(&at)?;
    Ok(torn)
}

fn torn(path: &Path, journal: &Journal) -> io::Result<Option<Torn>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let len = file.metadata()?.len();
    let pre_len = journal.pre_len;
    if len < pre_len {
        return Ok(Some(Torn::Left));
    }
    if len == pre_len {
        return Ok(None);
    }
    let tail = durable::TAIL;
    let end = read_range(&mut file, len.saturating_sub(tail)..len)?;
    let ended = startxref(&end).is_some_and(|(at, rest)| {
        let rest = rest.trim_ascii_start();
        at >= journal.start && rest.starts_with(b"%%EOF")
    });
    if ended {
        return Ok(None);
    }
    let from = pre_len.saturating_sub(tail);
    let before = read_range(&mut file, from..pre_len)?;
    let was = FileStamp::ending(pre_len, journal.stamp.mtime, from, &before, &mut |at| {
        read_range(&mut file, at)
    })?;
    if was != journal.stamp {
        return Ok(Some(Torn::Left));
    }
    drop(file);
    cut_back(path, journal)?;
    Ok(Some(Torn::Undone { len: pre_len }))
}

#[cfg(test)]
mod tests {
    use std::io::{Seek as _, SeekFrom};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::serialize::durable::tests::far_section;

    fn scratch() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("sheaf-update-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn update(len: usize) -> Vec<u8> {
        format!(
            "% an update\nxref\n0 1\n0000000000 65535 f \ntrailer\n<<>>\nstartxref\n{}\n%%EOF\n",
            len + 12
        )
        .into_bytes()
    }

    fn stamped(path: &Path, bytes: &[u8]) -> FileStamp {
        fs::write(path, bytes).unwrap();
        FileStamp::read(path).unwrap()
    }

    #[test]
    fn an_update_goes_after_the_file_and_leaves_no_journal() {
        let dir = scratch();
        let path = dir.join("a.pdf");
        let (_, original) = far_section();
        let base = stamped(&path, &original);
        let section = update(original.len());
        let now = append_in_place(&path, &base, &section).unwrap();
        assert_eq!(fs::read(&path).unwrap(), [&original[..], &section].concat());
        assert_eq!(now, FileStamp::read(&path).unwrap());
        assert!(!journal_path(&path).exists());
        assert_eq!(undo_torn(&path).unwrap(), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_another_program_changed_is_left_alone() {
        let dir = scratch();
        let path = dir.join("a.pdf");
        let (at, original) = far_section();
        for how in ["appended", "its last section"] {
            let base = stamped(&path, &original);
            let mut file = File::options().write(true).open(&path).unwrap();
            match how {
                "appended" => {
                    file.seek(SeekFrom::End(0)).unwrap();
                    file.write_all(b"\n").unwrap();
                }
                _ => {
                    file.seek(SeekFrom::Start(at + 7)).unwrap();
                    file.write_all(b"2").unwrap();
                    file.set_modified(base.mtime).unwrap();
                }
            }
            drop(file);
            let theirs = fs::read(&path).unwrap();
            let refused = append_in_place(&path, &base, &update(original.len()));
            assert!(matches!(refused, Err(WriteError::Changed { .. })), "{how}");
            assert_eq!(fs::read(&path).unwrap(), theirs, "{how}");
            assert!(!journal_path(&path).exists(), "{how}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_torn_update_is_cut_back_when_the_file_opens() {
        let dir = scratch();
        let path = dir.join("a.pdf");
        let (_, original) = far_section();
        let base = stamped(&path, &original);
        let section = update(original.len());
        tear_next(20);
        assert!(append_in_place(&path, &base, &section).is_err());
        assert_eq!(
            fs::read(&path).unwrap(),
            [&original[..], &section[..20]].concat()
        );
        assert!(journal_path(&path).exists());
        let undone = undo_torn(&path).unwrap();
        assert_eq!(
            undone,
            Some(Torn::Undone {
                len: original.len() as u64
            })
        );
        assert!(undone.unwrap().to_string().contains("didn't finish"));
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(base.holds(&path).unwrap());
        assert!(!journal_path(&path).exists());
        assert_eq!(undo_torn(&path).unwrap(), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_append_its_writer_holds_is_left_alone() {
        let dir = scratch();
        let path = dir.join("a.pdf");
        let (_, original) = far_section();
        let base = stamped(&path, &original);
        let section = update(original.len());
        let journal = Journal {
            pre_len: base.len,
            stamp: base,
            start: startxref(&section).unwrap().0,
        };
        let held = write_journal(&journal_path(&path), &journal).unwrap();
        let part = [&original[..], &section[..20]].concat();
        fs::write(&path, &part).unwrap();
        assert_eq!(undo_torn(&path).unwrap(), None);
        assert_eq!(fs::read(&path).unwrap(), part);
        assert!(journal_path(&path).exists());
        drop(held);
        let undone = undo_torn(&path).unwrap();
        assert!(matches!(undone, Some(Torn::Undone { .. })));
        assert_eq!(fs::read(&path).unwrap(), original);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_torn_append_through_a_link_is_undone_by_the_file_s_own_name() {
        let dir = scratch();
        let real = dir.join("real");
        fs::create_dir_all(&real).unwrap();
        let path = real.join("a.pdf");
        let (_, original) = far_section();
        fs::write(&path, &original).unwrap();
        let link = dir.join("link.pdf");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        let base = FileStamp::read(&link).unwrap();
        tear_next(20);
        assert!(append_in_place(&link, &base, &update(original.len())).is_err());
        assert!(journal_path(&path).exists() && !journal_path(&link).exists());
        let undone = undo_torn(&path).unwrap();
        assert!(matches!(undone, Some(Torn::Undone { .. })));
        assert_eq!(fs::read(&path).unwrap(), original);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_journal_undoes_only_its_own_torn_update() {
        let dir = scratch();
        let path = dir.join("a.pdf");
        let (_, original) = far_section();
        let section = update(original.len());
        let journal = |base: &FileStamp| Journal {
            pre_len: base.len,
            stamp: *base,
            start: startxref(&section).unwrap().0,
        };
        let cases = [
            ("finished", [&original[..], &section].concat(), None),
            (
                "shorter",
                original[..original.len() - 1].to_vec(),
                Some(Torn::Left),
            ),
            (
                "changed before",
                {
                    let mut theirs = [&original[..], &section[..20]].concat();
                    theirs[original.len() - 3] = b'?';
                    theirs
                },
                Some(Torn::Left),
            ),
        ];
        for (how, now, want) in cases {
            let base = stamped(&path, &original);
            fs::write(journal_path(&path), journal(&base).bytes().unwrap()).unwrap();
            fs::write(&path, &now).unwrap();
            assert_eq!(undo_torn(&path).unwrap(), want, "{how}");
            assert_eq!(fs::read(&path).unwrap(), now, "{how}");
            assert!(!journal_path(&path).exists(), "{how}");
        }
        let base = stamped(&path, &original);
        let cut = journal(&base).bytes().unwrap();
        fs::write(journal_path(&path), &cut[..cut.len() - 2]).unwrap();
        assert_eq!(undo_torn(&path).unwrap(), None);
        assert!(!journal_path(&path).exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
