//! Which libpdfium sheaf-render binds, by what is where: one build, said
//! (`tests/pdfium_build.rs` holds which), never another library unsaid.
//!
//! - `$PDFIUM_LIB_DIR` set: that directory's library or none.
//! - Unset: the first directory of the search that holds a library
//!   decides. A library there that doesn't bind, as another build's
//!   doesn't, is refused by name and not passed over for one further on.
//!
//! A process binds PDFium once, so each case is a process of its own: this
//! binary run again for [`open_an_engine`] alone, with the case's
//! directory and environment, saying what came of it on a line.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

use std::path::{Path, PathBuf};
use std::process::Command;

use pdfium_render::prelude::Pdfium;
use sheaf_doc::forms::fixture::Form;
use sheaf_render::Renderer;
use sheaf_render::pdfium::PdfiumEngine;

/// What starts the line [`open_an_engine`] says its outcome on.
const SAID: &str = "library_search: ";

#[test]
#[ignore = "a case's process: the tests of this file run it"]
fn open_an_engine() {
    match PdfiumEngine::open_bytes(Form::new(1).bytes().into(), None) {
        Ok(engine) => println!("{SAID}opened, {} page", engine.page_count()),
        Err(e) => println!("{SAID}refused: {}", format!("{e:#}").replace('\n', " ")),
    }
}

/// An empty directory for the case `name`.
fn case_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("library_search")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    dir
}

/// What a process says of opening an engine, run in `cwd` with
/// `$PDFIUM_LIB_DIR` as `lib_dir` gives it (unset for `None`).
fn said_in(cwd: &Path, lib_dir: Option<&Path>) -> String {
    let mut child = Command::new(std::env::current_exe().expect("this binary's path"));
    child
        .args(["--ignored", "--exact", "open_an_engine", "--nocapture"])
        .current_dir(cwd)
        .env_remove("PDFIUM_LIB_DIR");
    if let Some(dir) = lib_dir {
        child.env("PDFIUM_LIB_DIR", dir);
    }
    let out = child.output().expect("running this binary again");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let said = (stdout.lines()).find_map(|line| {
        line.find(SAID)
            .map(|at| line[at + SAID.len()..].to_string())
    });
    said.unwrap_or_else(|| {
        panic!(
            "the case's process said nothing ({}):\n{stdout}\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

/// A file named as libpdfium is in `dir`, made with its directories, that
/// no loader takes for a library: as near as a test comes to another
/// build's, which loads and lacks a function.
fn not_a_library(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    let lib = Pdfium::pdfium_platform_library_name_at_path(dir);
    std::fs::write(&lib, b"not a library").unwrap_or_else(|e| panic!("{}: {e}", lib.display()));
    lib
}

/// `vendor/pdfium/<os>-<arch>`, where the fetch script puts the library.
fn vendored() -> String {
    format!(
        "vendor/pdfium/{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

#[test]
fn with_nothing_in_the_way_the_fetched_library_is_bound() {
    // From a directory with no `vendor/`, the source tree's is found; and
    // `$PDFIUM_LIB_DIR` set but empty is unset.
    let cwd = case_dir("nothing");
    assert_eq!(said_in(&cwd, None), "opened, 1 page");
    assert_eq!(said_in(&cwd, Some(Path::new(""))), "opened, 1 page");
}

#[test]
fn a_set_lib_dir_binds_its_library_or_none() {
    // The source tree's `vendor/` holds a library that binds (the case
    // above); with the variable naming a directory that has none, or one
    // that doesn't bind, it is not gone on to.
    let cwd = case_dir("lib_dir");
    for (name, has_one) in [("empty", false), ("stale", true)] {
        let dir = cwd.join(name);
        let lib = match has_one {
            true => not_a_library(&dir),
            false => {
                std::fs::create_dir_all(&dir).unwrap();
                Pdfium::pdfium_platform_library_name_at_path(&dir)
            }
        };
        let said = said_in(&cwd, Some(&dir));
        assert!(
            said.starts_with("refused: $PDFIUM_LIB_DIR is ")
                && said.contains(&format!("{} doesn't bind", lib.display())),
            "{name}: {said}"
        );
    }
}

#[test]
fn a_library_that_is_there_and_doesnt_bind_is_refused_not_passed_over() {
    // The cwd's `vendor/` comes before the source tree's in the search:
    // with a library there that doesn't bind, the source tree's (which
    // does) isn't taken in its place.
    let cwd = case_dir("stale");
    let lib = not_a_library(&cwd.join(vendored()));
    let named = Path::new(&vendored()).join(lib.file_name().unwrap());
    let said = said_in(&cwd, None);
    assert!(
        said.starts_with(&format!("refused: {} doesn't bind", named.display())),
        "{said}"
    );
    assert!(said.contains("scripts/fetch-pdfium.sh"), "{said}");

    // A directory there with no library in it is passed over.
    std::fs::remove_file(&lib).unwrap();
    assert_eq!(said_in(&cwd, None), "opened, 1 page");
}
