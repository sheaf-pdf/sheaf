//! Where `bind` looks for libpdfium: `$PDFIUM_LIB_DIR` alone when it is
//! set, else the bundle's directories, then `vendor/`, then the system.
//! What `bind` does with a library it finds, a process each, is
//! `tests/library_search.rs`'s.

use super::*;

#[test]
fn a_set_pdfium_lib_dir_is_searched_alone() {
    assert_eq!(
        search(
            Some("/opt/pdfium-7881".into()),
            Some(Path::new("/usr/bin")),
            Path::new("vendor/pdfium/linux-x86_64"),
            Path::new("/src/sheaf"),
        ),
        Search::Only("/opt/pdfium-7881".into())
    );
}

#[test]
fn set_but_empty_is_unset() {
    assert_eq!(chosen(None), None);
    assert_eq!(chosen(Some("".into())), None);
    assert_eq!(
        chosen(Some("/opt/pdfium-7881".into())),
        Some("/opt/pdfium-7881".into())
    );
}

#[test]
fn unset_it_is_the_bundle_then_vendor() {
    let vendored = Path::new("vendor/pdfium/linux-x86_64");
    let dirs = |exe_dir: Option<&Path>| search(None, exe_dir, vendored, Path::new("/src/sheaf"));
    assert_eq!(
        dirs(Some(Path::new("/opt/sheaf"))),
        Search::Dirs(vec![
            "/opt/sheaf/lib".into(),
            "/opt/sheaf".into(),
            "vendor/pdfium/linux-x86_64".into(),
            "/src/sheaf/vendor/pdfium/linux-x86_64".into(),
        ])
    );
    assert_eq!(
        dirs(None),
        Search::Dirs(vec![
            "vendor/pdfium/linux-x86_64".into(),
            "/src/sheaf/vendor/pdfium/linux-x86_64".into(),
        ])
    );
}
