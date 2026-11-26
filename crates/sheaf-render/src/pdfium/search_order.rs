//! Where `bind` looks for libpdfium: `$PDFIUM_LIB_DIR` alone when it is
//! set, else the bundle's directories, then `vendor/`, then the system.

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
fn unset_or_empty_it_is_the_bundle_then_vendor() {
    let vendored = Path::new("vendor/pdfium/linux-x86_64");
    let dirs = |lib_dir: Option<OsString>, exe_dir: Option<&Path>| {
        search(lib_dir, exe_dir, vendored, Path::new("/src/sheaf"))
    };
    let bundled = Search::Dirs(vec![
        "/opt/sheaf/lib".into(),
        "/opt/sheaf".into(),
        "vendor/pdfium/linux-x86_64".into(),
        "/src/sheaf/vendor/pdfium/linux-x86_64".into(),
    ]);
    assert_eq!(dirs(None, Some(Path::new("/opt/sheaf"))), bundled);
    assert_eq!(
        dirs(Some("".into()), Some(Path::new("/opt/sheaf"))),
        bundled
    );
    assert_eq!(
        dirs(None, None),
        Search::Dirs(vec![
            "vendor/pdfium/linux-x86_64".into(),
            "/src/sheaf/vendor/pdfium/linux-x86_64".into(),
        ])
    );
}
