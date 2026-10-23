//! Documents with no file yet (M5 proposal 2): what Create from Scans
//! streams its pages into, kept in memory until Save As gives it a file.
//! Each is named for what made it and numbered from 1, the first number no
//! view claims, in this sheaf or another, and no crash left a sidecar of
//! ("Untitled scan 1", "Untitled scan 2"): its title, and the name Save As
//! suggests. Save As starts in the folder of what it was made from, and
//! its crash-recovery sidecar is in the state folder
//! ([`sheaf_ops::dirs::recovery_dir`]), since it has no file to sit
//! beside; the next launch offers it back under its name. The name's
//! claim ([`Claim`]) is the view's for its life, and each write of the
//! sidecar's in flight for the write's, so no other document takes the
//! name, nor another sheaf the sidecar, before they're done.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sheaf_doc::recovery::Claim;

/// A document no file holds yet.
#[derive(Debug)]
pub(crate) struct Untitled {
    /// What it's called: its title, and the name Save As suggests.
    pub(crate) name: String,
    /// The folder Save As starts in: none when it was made from no files.
    pub(crate) dir: PathBuf,
    /// The claim on its name and crash-recovery sidecar: none when the
    /// state folder can't keep one, so it has no sidecar.
    pub(crate) claim: Option<Arc<Claim>>,
}

/// The first name of `kind` ("Untitled scan") that no view claims and no
/// sidecar in the state folder has (a crash's, which the launch offers
/// back under its name, and a new document's would replace), claimed.
/// Unclaimed when the state folder can't keep claims: the first name with
/// no sidecar, which then keeps none.
pub(crate) fn next(kind: &str) -> (String, Option<Claim>) {
    claim_next(&sheaf_ops::dirs::recovery_dir(), kind)
}

/// [`next`] in `dir`.
fn claim_next(dir: &Path, kind: &str) -> (String, Option<Claim>) {
    (1..)
        .map(|n| format!("{kind} {n}"))
        .find_map(
            |name| match sheaf_doc::recovery::claim_untitled(dir, &name) {
                // Looked for once it's held, so no crash's sidecar comes
                // meanwhile.
                Ok(Some(claim)) => (!claim.sidecar().exists()).then(|| (name, Some(claim))),
                Ok(None) => None,
                Err(e) => {
                    log::warn!("{}: {e}; {name} keeps no recovery data", dir.display());
                    let kept = sheaf_doc::recovery::untitled_path(dir, &name).exists();
                    (!kept).then_some((name, None))
                }
            },
        )
        .expect("a number is free")
}

/// The untitled document Create from Scans makes of `files`: the next
/// "Untitled scan" ([`next`]), saved by default beside the first of them.
pub(crate) fn for_scan(files: &[PathBuf]) -> Untitled {
    let (name, claim) = next("Untitled scan");
    let dir = (files.first())
        .and_then(|f| f.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    Untitled {
        name,
        dir,
        claim: claim.map(Arc::new),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name takes the first number no claim holds and no sidecar has: 1
    /// with none, the gap once one is let go, and the next after the last
    /// otherwise. Another kind's names, a file's and other files in the
    /// folder take none; a sidecar a crash left in the state folder takes
    /// its name, as a claim does.
    #[test]
    fn untitled_names_take_the_first_free_number() {
        let kind = "Untitled scan";
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let dir = dir.path();
        let next = || claim_next(dir, kind);
        let (one, first) = next();
        assert_eq!(one, "Untitled scan 1");
        let (two, second) = next();
        assert_eq!(two, "Untitled scan 2");
        let (three, _third) = next();
        assert_eq!(three, "Untitled scan 3");
        drop(second);
        assert_eq!(next().0, "Untitled scan 2");
        let first = first.unwrap();
        assert_eq!(first.name(), "Untitled scan 1");
        assert_eq!(first.sidecar(), dir.join("Untitled scan 1.sheaf-recovery"));
        for other in ["Untitled 2", "Untitled scan 2.pdf", "Untitled scan 2"] {
            std::fs::write(dir.join(other), b"").unwrap();
        }
        assert_eq!(next().0, "Untitled scan 2");
        let left = sheaf_doc::recovery::untitled_path(dir, "Untitled scan 2");
        std::fs::write(left, b"").unwrap();
        assert_eq!(next().0, "Untitled scan 4");
        drop(first);
        assert_eq!(next().0, "Untitled scan 1");
    }
}
