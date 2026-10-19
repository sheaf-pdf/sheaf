//! A file replaced whole, durably: written beside it under a temp name no
//! other write uses, given its permissions and fsynced, checked against
//! what sheaf read, renamed over it, and its directory fsynced. A crash
//! leaves the old file or the new one, never part of either, and a file
//! another program changed since sheaf read it is left alone (M10.5
//! §11.7, bug 2). A copy of a file with an update after it (Save As of a
//! signed document) is put in place the same way, the file it copies
//! checked before and after.
//!
//! What sheaf read is a [`FileStamp`]: the file's length and time, and
//! hashes of its last 64 KiB and of the 4 KiB its last `startxref` points
//! at.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How much of a file's end its stamp hashes.
pub(crate) const TAIL: u64 = 64 * 1024;

/// How much of a file its stamp hashes at its last `startxref`'s target.
const WINDOW: u64 = 4 * 1024;

/// The longest file name the filesystems sheaf saves to take: 255 bytes
/// (ext4, APFS) or 255 UTF-16 units (NTFS), which a name never has more
/// of than it has UTF-8 bytes.
const NAME_MAX: usize = 255;

/// What a temp name adds to the file's: a `.` before it, and the random
/// part and the suffix after.
const TEMP_EXTRA: usize = ".".len() + ".0123456789abcdef.sheaf-tmp".len();

/// A file as sheaf read it: enough to tell that another program changed it
/// since.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    pub len: u64,
    pub mtime: SystemTime,
    /// xxh3-128 of its last 64 KiB, or of all of it when it's shorter.
    pub tail: u128,
    /// xxh3-128 of up to 4 KiB at the position its last `startxref` says,
    /// cut at its end: its last cross-reference section, which on a large
    /// file starts further back than the tail, so a change there that keeps
    /// the length shows too. Of nothing when the tail has no `startxref`.
    pub window: u128,
}

impl FileStamp {
    /// The stamp of `bytes`, read from a file whose metadata, taken before
    /// the read, is `meta`.
    pub fn of(meta: &Metadata, bytes: &[u8]) -> io::Result<FileStamp> {
        let len = bytes.len() as u64;
        FileStamp::ending(len, meta.modified()?, 0, bytes, &mut |at| {
            Ok(bytes[at.start as usize..at.end as usize].to_vec())
        })
    }

    /// The file at `path` as it is now, from its metadata, its tail and the
    /// bytes at its last `startxref`.
    pub fn read(path: &Path) -> io::Result<FileStamp> {
        Ok(FileStamp::read_keeping_tail(path)?.0)
    }

    /// [`read`](Self::read), and the tail it read, from where it is in the
    /// file.
    pub(crate) fn read_keeping_tail(path: &Path) -> io::Result<(FileStamp, u64, Vec<u8>)> {
        let mut file = File::open(path)?;
        let meta = file.metadata()?;
        let len = meta.len();
        let from = len.saturating_sub(TAIL);
        // Short when another program truncates it meanwhile: then the
        // hashes differ.
        let tail = read_range(&mut file, from..len)?;
        let stamp = FileStamp::ending(len, meta.modified()?, from, &tail, &mut |at| {
            read_range(&mut file, at)
        })?;
        Ok((stamp, from, tail))
    }

    /// The stamp of a file `len` long, modified at `mtime`, whose bytes
    /// from `from` to its end are `end` (its last 64 KiB at least, or all
    /// of it), `read` giving any other range of it.
    pub(crate) fn ending(
        len: u64,
        mtime: SystemTime,
        from: u64,
        end: &[u8],
        read: &mut dyn FnMut(Range<u64>) -> io::Result<Vec<u8>>,
    ) -> io::Result<FileStamp> {
        let tail = &end[end.len().saturating_sub(TAIL as usize)..];
        let window = match startxref(tail) {
            None => sheaf_core::checksum(&[]),
            Some((at, _)) => {
                let at = at.min(len)..at.saturating_add(WINDOW).min(len);
                match at.start.checked_sub(from) {
                    Some(start) => {
                        let start = start as usize;
                        let end = &end[start.min(end.len())..];
                        sheaf_core::checksum(&end[..end.len().min((at.end - at.start) as usize)])
                    }
                    None => sheaf_core::checksum(&read(at)?),
                }
            }
        };
        Ok(FileStamp {
            len,
            mtime,
            tail: sheaf_core::checksum(tail),
            window,
        })
    }

    /// Whether the file at `path` is still the one this stamp is of. A
    /// file that's gone isn't.
    pub fn holds(&self, path: &Path) -> io::Result<bool> {
        match FileStamp::read(path) {
            Ok(now) => Ok(now == *self),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// The stamp as a recovery sidecar or an append's journal says it: the
    /// tail's hash, the window's, then the time in nanoseconds since 1970,
    /// in 32 hex digits each. None for a time before.
    pub fn hex(&self) -> Option<String> {
        let since = self.mtime.duration_since(SystemTime::UNIX_EPOCH).ok()?;
        Some(format!(
            "{:032x}{:032x}{:032x}",
            self.tail,
            self.window,
            since.as_nanos()
        ))
    }

    /// The stamp of a file `len` long that [`hex`](Self::hex) wrote as
    /// `hex`.
    pub fn from_hex(len: u64, hex: &str) -> Option<FileStamp> {
        if hex.len() != 96 || !hex.is_ascii() {
            return None;
        }
        let (tail, rest) = hex.split_at(32);
        let (window, nanos) = rest.split_at(32);
        let nanos = u128::from_str_radix(nanos, 16).ok()?;
        let secs = u64::try_from(nanos / 1_000_000_000).ok()?;
        let since = Duration::new(secs, (nanos % 1_000_000_000) as u32);
        Some(FileStamp {
            len,
            mtime: SystemTime::UNIX_EPOCH.checked_add(since)?,
            tail: u128::from_str_radix(tail, 16).ok()?,
            window: u128::from_str_radix(window, 16).ok()?,
        })
    }
}

/// What the last `startxref` in `end` says, and the bytes after its number:
/// None when there's none, or no number after it.
pub(crate) fn startxref(end: &[u8]) -> Option<(u64, &[u8])> {
    const KEYWORD: &[u8] = b"startxref";
    let at = end.windows(KEYWORD.len()).rposition(|w| w == KEYWORD)?;
    let rest = &end[at + KEYWORD.len()..];
    let rest = &rest[rest
        .iter()
        .take_while(|b| b"\0\t\n\x0c\r ".contains(b))
        .count()..];
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    let n = std::str::from_utf8(&rest[..digits]).ok()?.parse().ok()?;
    Some((n, &rest[digits..]))
}

/// The bytes of `file` in `range`, as many as it has.
pub(crate) fn read_range(file: &mut File, range: Range<u64>) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(range.start))?;
    let len = range.end.saturating_sub(range.start);
    let mut bytes = Vec::with_capacity(len as usize);
    file.take(len).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Whether `a` and `b` name one file: the same file once links and `..`
/// are followed, or, when either isn't there to follow, the same path.
pub fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Why a durable write left the file as it was.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// The file isn't the one the write was to replace: another program
    /// changed, replaced or removed it.
    #[error("{} changed on disk since sheaf read it", .path.display())]
    Changed { path: PathBuf },
    #[error("writing {}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Replace the file at `path` with `bytes`, or create it. With `expect`,
/// only while the file is still the one stamped: otherwise nothing is
/// written ([`WriteError::Changed`]). A symbolic link's target is replaced
/// and the link kept, and the new file has the old one's permissions. No
/// temp file is left, whatever happens.
pub fn write(path: &Path, bytes: &[u8], expect: Option<&FileStamp>) -> Result<(), WriteError> {
    let io_at = |path: &Path| {
        let path = path.to_path_buf();
        move |source| WriteError::Io { path, source }
    };
    let target = resolve(path).map_err(io_at(path))?;
    let (tmp, file) = temp_beside(&target).map_err(io_at(&target))?;
    match replace(file, &tmp, &target, bytes, expect) {
        Ok(true) => {}
        Ok(false) => {
            let _ = fs::remove_file(&tmp);
            return Err(WriteError::Changed {
                path: path.to_path_buf(),
            });
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            return Err(io_at(&target)(e));
        }
    }
    #[cfg(unix)]
    sync_dir(dir_of(&target));
    Ok(())
}

/// Write `bytes` to `file`, the temp `tmp`, and rename it over `target`
/// when `expect` still holds there: whether it did.
fn replace(
    mut file: File,
    tmp: &Path,
    target: &Path,
    bytes: &[u8],
    expect: Option<&FileStamp>,
) -> io::Result<bool> {
    file.write_all(bytes)?;
    keep_permissions(&file, target)?;
    file.sync_all()?;
    drop(file);
    if let Some(stamp) = expect
        && !stamp.holds(target)?
    {
        return Ok(false);
    }
    fs::rename(tmp, target)?;
    Ok(true)
}

/// A copy of the file at `from` with `section` after it, put at `to` as
/// [`write()`] puts a file: only while `from` is still the file `stamp`
/// says, before the copy and after it, and when `sum` is given, only when
/// the bytes copied have that checksum. Otherwise nothing is written
/// ([`WriteError::Changed`], of `from`). What `to` is then, and the
/// checksum of all of it, hashed as it's copied.
pub fn copy_appended(
    from: &Path,
    stamp: &FileStamp,
    sum: Option<u128>,
    to: &Path,
    section: &[u8],
) -> Result<(FileStamp, sheaf_core::Checksum), WriteError> {
    let changed = || WriteError::Changed {
        path: from.to_path_buf(),
    };
    let io_at = |path: &Path| {
        let path = path.to_path_buf();
        move |source| WriteError::Io { path, source }
    };
    if !stamp.holds(from).map_err(io_at(from))? {
        return Err(changed());
    }
    let target = resolve(to).map_err(io_at(to))?;
    let (tmp, file) = temp_beside(&target).map_err(io_at(&target))?;
    let copied = copy_into(file, from, stamp, sum, section, &target);
    let (file, copied) = match copied {
        Ok(Some(copied)) => copied,
        Ok(None) => {
            let _ = fs::remove_file(&tmp);
            return Err(changed());
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            return Err(io_at(&target)(e));
        }
    };
    drop(file);
    let renamed = match stamp.holds(from) {
        Ok(true) => fs::rename(&tmp, &target).map(|()| true),
        held => held,
    };
    match renamed {
        Ok(true) => {}
        Ok(false) => {
            let _ = fs::remove_file(&tmp);
            return Err(changed());
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            return Err(io_at(&target)(e));
        }
    }
    #[cfg(unix)]
    sync_dir(dir_of(&target));
    // Its time once it's closed and in place: Windows sets a write's time
    // as the handle closes.
    let (mut stamp, sum) = copied;
    stamp.mtime =
        (fs::metadata(&target).and_then(|meta| meta.modified())).map_err(io_at(&target))?;
    Ok((stamp, sum))
}

/// [`copy_appended`]'s copy into `file`, the temp beside `target`, synced:
/// the file, its stamp and its checksum, or None when `from` isn't as
/// long as `stamp` says or its bytes haven't checksum `sum`.
fn copy_into(
    mut file: File,
    from: &Path,
    stamp: &FileStamp,
    sum: Option<u128>,
    section: &[u8],
    target: &Path,
) -> io::Result<Option<(File, (FileStamp, sheaf_core::Checksum))>> {
    let mut source = File::open(from)?;
    let mut hash = sheaf_core::Checksum::new();
    let mut buf = vec![0; 1 << 20];
    // The last bytes copied, for the stamp.
    let mut end: Vec<u8> = Vec::new();
    let mut len = 0u64;
    loop {
        let n = match source.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let bytes = &buf[..n];
        hash.write(bytes);
        file.write_all(bytes)?;
        len += n as u64;
        let tail = TAIL as usize;
        if n >= tail {
            end.clear();
            end.extend_from_slice(&bytes[n - tail..]);
        } else {
            end.extend_from_slice(bytes);
            end.drain(..end.len().saturating_sub(tail));
        }
    }
    if len != stamp.len || sum.is_some_and(|sum| hash.finish() != sum) {
        return Ok(None);
    }
    hash.write(section);
    file.write_all(section)?;
    keep_permissions(&file, target)?;
    file.sync_all()?;
    let from = len - end.len() as u64;
    end.extend_from_slice(section);
    let total = len + section.len() as u64;
    // Its time is taken once it's in place (`copy_appended`).
    let now = FileStamp::ending(total, SystemTime::UNIX_EPOCH, from, &end, &mut |at| {
        read_range(&mut file, at)
    })?;
    Ok(Some((file, (now, hash))))
}

/// `file`, a temp to rename over `target`, given `target`'s permissions
/// when there's a file there. Windows' only permission, read-only, would
/// refuse the rename anyway.
fn keep_permissions(file: &File, target: &Path) -> io::Result<()> {
    #[cfg(unix)]
    if let Ok(meta) = fs::metadata(target) {
        file.set_permissions(meta.permissions())?;
    }
    #[cfg(not(unix))]
    let _ = (file, target);
    Ok(())
}

/// A new file beside `target`, `.<name>.<random>.sheaf-tmp`, the name cut
/// short where the whole would be longer than a file name can be: created
/// only when no other write has that name, so concurrent saves never
/// share one.
pub(crate) fn temp_beside(target: &Path) -> io::Result<(PathBuf, File)> {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let mut end = name.len().min(NAME_MAX - TEMP_EXTRA);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    let name = &name[..end];
    for _ in 0..16 {
        let tmp = dir_of(target).join(format!(".{name}.{:016x}.sheaf-tmp", fastrand::u64(..)));
        let opened = (OpenOptions::new().read(true).write(true))
            .create_new(true)
            .open(&tmp);
        match opened {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            opened => return opened.map(|file| (tmp, file)),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free temp name beside the file",
    ))
}

/// `path`, through any symbolic links, to the file they name: what a
/// rename replaces, so the links stay.
pub(crate) fn resolve(path: &Path) -> io::Result<PathBuf> {
    let mut path = path.to_path_buf();
    // As many links as Linux follows.
    for _ in 0..40 {
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                path = dir_of(&path).join(fs::read_link(&path)?);
            }
            _ => return Ok(path),
        }
    }
    Err(io::Error::other("too many levels of symbolic links"))
}

/// The directory `path` is in.
pub(crate) fn dir_of(path: &Path) -> &Path {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    }
}

/// Make a rename, a new file or a removal in `dir` survive a crash, as far
/// as `dir` lets it. The file is written by then, so a directory that
/// can't be read or synced is a warning, not a failed write. A filesystem
/// that can't sync a directory says so (EINVAL, ENOTSUP): its entries are
/// as durable as it makes them.
#[cfg(unix)]
pub(crate) fn sync_dir(dir: &Path) {
    match File::open(dir).and_then(|dir| dir.sync_all()) {
        Ok(()) => {}
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::InvalidInput | io::ErrorKind::Unsupported
            ) => {}
        Err(e) => log::warn!("syncing {}: {e}", dir.display()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeSet;
    use std::sync::Barrier;
    use std::time::Duration;

    use super::*;

    /// An empty directory of the test's own.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sheaf-durable-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The temp files left in `dir`.
    fn temps(dir: &Path) -> Vec<PathBuf> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().ends_with(".sheaf-tmp"))
            .collect()
    }

    /// A file whose last section starts more than 64 KiB from its end, as a
    /// large file's cross-reference table does: what its `startxref` says,
    /// and its bytes.
    pub(crate) fn far_section() -> (u64, Vec<u8>) {
        let mut bytes = b"%PDF-1.7\n".to_vec();
        let at = bytes.len() as u64;
        bytes.extend_from_slice(b"xref\n0 1\n0000000000 65535 f \n");
        for i in 0..5000 {
            bytes.extend_from_slice(format!("%% filler line {i:08}\n").as_bytes());
        }
        bytes.extend_from_slice(format!("trailer\n<<>>\nstartxref\n{at}\n%%EOF\n").as_bytes());
        assert!(bytes.len() as u64 - at > TAIL);
        (at, bytes)
    }

    /// A stamp as a sidecar or a journal says it reads back as it was, and
    /// one of another length or a time before 1970 doesn't.
    #[test]
    fn a_stamp_reads_back_as_written() {
        let stamp = FileStamp {
            len: 12345,
            mtime: SystemTime::UNIX_EPOCH + Duration::new(1_760_000_000, 123_456_789),
            tail: 0x0123_4567_89ab_cdef_fedc_ba98_7654_3210,
            window: 0xfedc_ba98_7654_3210_0123_4567_89ab_cdef,
        };
        let hex = stamp.hex().unwrap();
        assert_eq!(FileStamp::from_hex(12345, &hex), Some(stamp));
        assert_eq!(FileStamp::from_hex(12345, &hex[1..]), None);
        assert_eq!(FileStamp::from_hex(12345, &hex[32..]), None);
        let before = FileStamp {
            mtime: SystemTime::UNIX_EPOCH - Duration::from_secs(1),
            ..stamp
        };
        assert_eq!(before.hex(), None);
    }

    /// The window is the 4 KiB at the last `startxref`'s target, read where
    /// it is when that's further back than the tail, so a change there
    /// that keeps the length and the time shows; stamped from the bytes
    /// read, or read again, a file has one stamp.
    #[test]
    fn a_change_at_the_last_section_shows_however_far_back() {
        let dir = scratch("window");
        let path = dir.join("a.pdf");
        let (at, original) = far_section();
        fs::write(&path, &original).unwrap();
        let (bytes, meta) = sheaf_core::read_file_and_metadata(&path).unwrap();
        let stamp = FileStamp::of(&meta, &bytes).unwrap();
        assert_eq!(FileStamp::read(&path).unwrap(), stamp);
        let window = &original[at as usize..at as usize + WINDOW as usize];
        assert_eq!(stamp.window, sheaf_core::checksum(window));
        // A byte of the section, then one past its window.
        for (offset, changes) in [(at + 5, true), (at + WINDOW + 5, false)] {
            let mut file = File::options().write(true).open(&path).unwrap();
            file.seek(SeekFrom::Start(offset)).unwrap();
            file.write_all(b"9").unwrap();
            file.set_modified(stamp.mtime).unwrap();
            drop(file);
            let now = FileStamp::read(&path).unwrap();
            assert_eq!(
                (now.len, now.mtime, now.tail),
                (stamp.len, stamp.mtime, stamp.tail)
            );
            assert_eq!(now != stamp, changes, "{offset}");
            fs::write(&path, &original).unwrap();
            File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(stamp.mtime)
                .unwrap();
        }
        // A target past the end hashes nothing of it; no `startxref`, the
        // same.
        let empty = sheaf_core::checksum(&[]);
        for bytes in [&b"startxref\n99999\n%%EOF"[..], b"%PDF-1.7\n"] {
            fs::write(&path, bytes).unwrap();
            assert_eq!(FileStamp::read(&path).unwrap().window, empty);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// A copy of a file with a section after it, put in place as a write
    /// is, hashed as it's copied; refused, with nothing written, when the
    /// file isn't the one stamped or its bytes aren't the ones summed.
    #[test]
    fn a_copy_appends_only_to_the_file_it_was_stamped() {
        let dir = scratch("copy");
        let (from, to) = (dir.join("a.pdf"), dir.join("b.pdf"));
        let (_, original) = far_section();
        fs::write(&from, &original).unwrap();
        let stamp = FileStamp::read(&from).unwrap();
        let sum = sheaf_core::checksum(&original);
        let section = b"\n%% a section\nstartxref\n9\n%%EOF\n";
        let (now, hash) = copy_appended(&from, &stamp, Some(sum), &to, section).unwrap();
        let written = fs::read(&to).unwrap();
        assert_eq!(written, [&original[..], section].concat());
        assert_eq!(now, FileStamp::read(&to).unwrap());
        assert_eq!(hash.finish(), sheaf_core::checksum(&written));
        assert_eq!(fs::read(&from).unwrap(), original);
        // Summed otherwise, or changed since it was stamped: refused.
        fs::remove_file(&to).unwrap();
        let refused = copy_appended(&from, &stamp, Some(sum ^ 1), &to, section);
        assert!(matches!(refused, Err(WriteError::Changed { path }) if path == from));
        let mut theirs = original.clone();
        theirs.push(b'\n');
        fs::write(&from, &theirs).unwrap();
        let refused = copy_appended(&from, &stamp, None, &to, section);
        assert!(matches!(refused, Err(WriteError::Changed { path }) if path == from));
        assert!(!to.exists() && temps(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_changed_before_the_rename_is_left_alone() {
        let dir = scratch("changed");
        let path = dir.join("a.pdf");
        // Longer than the tail a stamp hashes.
        let original: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        for how in ["appended", "a new tail", "touched"] {
            fs::write(&path, &original).unwrap();
            // As an open stamps it: from the bytes it read.
            let (bytes, meta) = sheaf_core::read_file_and_metadata(&path).unwrap();
            let stamp = FileStamp::of(&meta, &bytes).unwrap();
            assert_eq!(FileStamp::read(&path).unwrap(), stamp, "{how}");
            let mut file = File::options().write(true).open(&path).unwrap();
            match how {
                "appended" => {
                    file.seek(SeekFrom::End(0)).unwrap();
                    file.write_all(b"%% another program's update\n").unwrap();
                }
                // The same length, and the time put back: only the tail
                // tells.
                "a new tail" => {
                    file.seek(SeekFrom::End(-1)).unwrap();
                    file.write_all(b"!").unwrap();
                    file.set_modified(stamp.mtime).unwrap();
                }
                _ => file
                    .set_modified(stamp.mtime + Duration::from_secs(1))
                    .unwrap(),
            }
            drop(file);
            let theirs = fs::read(&path).unwrap();
            assert!(!stamp.holds(&path).unwrap(), "{how}");
            match write(&path, b"ours", Some(&stamp)) {
                Err(WriteError::Changed { path: p }) => assert_eq!(p, path, "{how}"),
                other => panic!("{how}: {other:?}"),
            }
            assert_eq!(fs::read(&path).unwrap(), theirs, "{how}");
            assert!(temps(&dir).is_empty(), "{how}");
        }
        // Gone is changed too, and isn't made again.
        let stamp = FileStamp::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(matches!(
            write(&path, b"ours", Some(&stamp)),
            Err(WriteError::Changed { .. })
        ));
        assert!(!path.exists());
        // A stamp that holds lets the write through.
        fs::write(&path, &original).unwrap();
        write(&path, b"ours", Some(&FileStamp::read(&path).unwrap())).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"ours");
        assert!(temps(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_saves_get_distinct_temp_names() {
        const SAVES: usize = 16;
        let dir = scratch("concurrent");
        let path = dir.join("a.pdf");
        fs::write(&path, b"before").unwrap();
        // Each of its own length, and long enough that the writes overlap.
        let inputs: Vec<Vec<u8>> = (0..SAVES).map(|i| vec![i as u8; (1 << 20) + i]).collect();
        let start = Barrier::new(SAVES);
        let (path_, start_) = (&path, &start);
        std::thread::scope(|s| {
            for input in &inputs {
                s.spawn(move || {
                    start_.wait();
                    write(path_, input, None).unwrap();
                });
            }
        });
        assert!(inputs.contains(&fs::read(&path).unwrap()), "a mix");
        assert!(temps(&dir).is_empty());
        // The names they take, all at once, all differ.
        let made: Vec<(PathBuf, File)> = std::thread::scope(|s| {
            let made: Vec<_> = (0..SAVES)
                .map(|_| {
                    s.spawn(move || {
                        start_.wait();
                        temp_beside(path_).unwrap()
                    })
                })
                .collect();
            made.into_iter().map(|t| t.join().unwrap()).collect()
        });
        let names: BTreeSet<_> = made.iter().map(|(tmp, _)| tmp.clone()).collect();
        assert_eq!(names.len(), SAVES);
        for tmp in &names {
            let name = tmp.file_name().unwrap().to_str().unwrap();
            let random = name
                .strip_prefix(".a.pdf.")
                .and_then(|n| n.strip_suffix(".sheaf-tmp"))
                .unwrap_or_else(|| panic!("{name}"));
            assert!(random.len() == 16 && random.bytes().all(|b| b.is_ascii_hexdigit()));
            fs::remove_file(tmp).unwrap();
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_long_name_gets_a_temp_name_that_fits() {
        let dir = scratch("long");
        // 250 and 253 bytes, the second of three-byte characters a cut
        // mustn't split.
        for name in ["a".repeat(246) + ".pdf", "文".repeat(83) + ".pdf"] {
            let path = dir.join(&name);
            fs::write(&path, b"before").unwrap();
            write(&path, b"after", Some(&FileStamp::read(&path).unwrap())).unwrap();
            assert_eq!(fs::read(&path).unwrap(), b"after");
            let (tmp, _) = temp_beside(&path).unwrap();
            let temp = tmp.file_name().unwrap().to_str().unwrap();
            assert!(temp.len() <= NAME_MAX, "{temp}");
            let kept = &temp[1..temp.len() + 1 - TEMP_EXTRA];
            assert!(kept.len() > 220 && name.starts_with(kept), "{temp}");
            fs::remove_file(&tmp).unwrap();
        }
        assert!(temps(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    /// A directory sheaf can write in but not read can't be synced: the
    /// file is replaced all the same, and the write is done.
    #[cfg(unix)]
    #[test]
    fn a_directory_that_cant_be_synced_still_saves() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = scratch("unreadable");
        let path = dir.join("a.pdf");
        fs::write(&path, b"before").unwrap();
        let stamp = FileStamp::read(&path).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o300)).unwrap();
        let wrote = write(&path, b"after", Some(&stamp));
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        wrote.unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"after");
        assert!(temps(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn permissions_are_preserved() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = scratch("permissions");
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o7777;
        for want in [0o640, 0o600] {
            let path = dir.join(format!("{want:o}.pdf"));
            fs::write(&path, b"before").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(want)).unwrap();
            write(&path, b"after", None).unwrap();
            assert_eq!(mode(&path), want, "{want:o}");
            assert_eq!(fs::read(&path).unwrap(), b"after");
        }
        // A new file gets what any new file gets here.
        let made = dir.join("made.pdf");
        fs::write(&made, b"").unwrap();
        let new = dir.join("new.pdf");
        write(&new, b"new", None).unwrap();
        assert_eq!(mode(&new), mode(&made));
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_file_stays_a_symlink() {
        use std::os::unix::fs::symlink;
        let dir = scratch("symlink");
        let real = dir.join("real.pdf");
        fs::write(&real, b"before").unwrap();
        let link = dir.join("link.pdf");
        symlink("real.pdf", &link).unwrap();
        // A link to the link, by its full path.
        let chain = dir.join("chain.pdf");
        symlink(&link, &chain).unwrap();
        let is_link = |path: &Path| fs::symlink_metadata(path).unwrap().is_symlink();
        write(&link, b"after", Some(&FileStamp::read(&link).unwrap())).unwrap();
        assert!(is_link(&link));
        assert_eq!(fs::read(&real).unwrap(), b"after");
        write(&chain, b"again", None).unwrap();
        assert!(is_link(&chain) && is_link(&link));
        assert_eq!(fs::read(&real).unwrap(), b"again");
        // A link to a file not there yet makes it.
        let ahead = dir.join("ahead.pdf");
        symlink("later.pdf", &ahead).unwrap();
        write(&ahead, b"later", None).unwrap();
        assert!(is_link(&ahead));
        assert_eq!(fs::read(dir.join("later.pdf")).unwrap(), b"later");
        assert!(temps(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
