//! A file replaced whole, durably: written beside it under a temp name no
//! other write uses, given its permissions and fsynced, checked against
//! what sheaf read, renamed over it, and its directory fsynced. A crash
//! leaves the old file or the new one, never part of either, and a file
//! another program changed since sheaf read it is left alone (M10.5
//! §11.7, bug 2).

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// How much of a file's end its stamp hashes.
const TAIL: u64 = 64 * 1024;

/// The longest file name the filesystems sheaf saves to take: 255 bytes
/// (ext4, APFS) or 255 UTF-16 units (NTFS), which a name never has more
/// of than it has UTF-8 bytes.
const NAME_MAX: usize = 255;

/// What a temp name adds to the file's: a `.` before it, and the random
/// part and the suffix after.
const TEMP_EXTRA: usize = ".".len() + ".0123456789abcdef.sheaf-tmp".len();

/// A file as sheaf read it: enough to tell that another program changed it
/// since. Slice 10 adds the hash at its last `startxref`'s target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    pub len: u64,
    pub mtime: SystemTime,
    /// xxh3-128 of its last 64 KiB, or of all of it when it's shorter.
    pub tail: u128,
}

impl FileStamp {
    /// The stamp of `bytes`, read from a file whose metadata, taken before
    /// the read, is `meta`.
    pub fn of(meta: &Metadata, bytes: &[u8]) -> io::Result<FileStamp> {
        let tail = bytes.len().saturating_sub(TAIL as usize);
        Ok(FileStamp {
            len: bytes.len() as u64,
            mtime: meta.modified()?,
            tail: sheaf_core::checksum(&bytes[tail..]),
        })
    }

    /// The file at `path` as it is now, from its metadata and its tail.
    pub fn read(path: &Path) -> io::Result<FileStamp> {
        let mut file = File::open(path)?;
        let meta = file.metadata()?;
        let len = meta.len();
        file.seek(SeekFrom::Start(len.saturating_sub(TAIL)))?;
        // Short when another program truncates it meanwhile: then the hash
        // differs.
        let mut tail = Vec::with_capacity(len.min(TAIL) as usize);
        file.take(TAIL).read_to_end(&mut tail)?;
        Ok(FileStamp {
            len,
            mtime: meta.modified()?,
            tail: sheaf_core::checksum(&tail),
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
    // Windows' only permission, read-only, would refuse the rename anyway.
    #[cfg(unix)]
    if let Ok(meta) = fs::metadata(target) {
        file.set_permissions(meta.permissions())?;
    }
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

/// A new file beside `target`, `.<name>.<random>.sheaf-tmp`, the name cut
/// short where the whole would be longer than a file name can be: created
/// only when no other write has that name, so concurrent saves never
/// share one.
fn temp_beside(target: &Path) -> io::Result<(PathBuf, File)> {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let mut end = name.len().min(NAME_MAX - TEMP_EXTRA);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    let name = &name[..end];
    for _ in 0..16 {
        let tmp = dir_of(target).join(format!(".{name}.{:016x}.sheaf-tmp", fastrand::u64(..)));
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
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
fn resolve(path: &Path) -> io::Result<PathBuf> {
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
fn dir_of(path: &Path) -> &Path {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    }
}

/// Make a rename in `dir` survive a crash, as far as `dir` lets it. The
/// file is replaced by then, so a directory that can't be read or synced
/// is a warning, not a failed write. A filesystem that can't sync a
/// directory says so (EINVAL, ENOTSUP): its renames are as durable as it
/// makes them.
#[cfg(unix)]
fn sync_dir(dir: &Path) {
    match File::open(dir).and_then(|dir| dir.sync_all()) {
        Ok(()) => {}
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::InvalidInput | io::ErrorKind::Unsupported
            ) => {}
        Err(e) => log::warn!("syncing {} after a rename in it: {e}", dir.display()),
    }
}

#[cfg(test)]
mod tests {
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
