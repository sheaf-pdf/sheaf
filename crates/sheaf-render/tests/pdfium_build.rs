//! One PDFium build: the build whose API pdfium-render binds (its
//! `pdfium_*` feature, in the workspace's Cargo.toml), the build
//! `scripts/fetch-pdfium.sh` fetches by default, and the build of the
//! library it fetched into `vendor/pdfium/<os>-<arch>/` (`BUILD=` in the
//! `VERSION` it keeps there) are one number. A move of the build edits the
//! feature and the script; this test names none.
//!
//! The binding refuses a library that lacks a function of its API, so on
//! every system CI runs this on, passing is also the proof that the
//! fetched library binds.
//!
//! With `$PDFIUM_LIB_DIR` set, a library chosen on purpose, the library
//! there is bound and the binding and the script are held alone.

use std::path::{Path, PathBuf};

use pdfium_render::prelude::version::PdfiumApiVersion;
use pdfium_render::prelude::*;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `default_build=<n>` in the script.
fn script_build() -> u32 {
    let path = root().join("scripts/fetch-pdfium.sh");
    let script = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let line = script
        .lines()
        .find_map(|l| l.trim().strip_prefix("default_build="))
        .unwrap_or_else(|| panic!("{} has no default_build= line", path.display()));
    line.trim()
        .parse()
        .unwrap_or_else(|e| panic!("{}: default_build={line}: {e}", path.display()))
}

/// `BUILD=<n>` in `<dir>/VERSION`.
fn fetched_build(dir: &Path) -> u32 {
    let path = dir.join("VERSION");
    let version = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "reading {}: {e}; an older scripts/fetch-pdfium.sh kept none: run it again",
            path.display()
        )
    });
    let line = version
        .lines()
        .find_map(|l| l.trim().strip_prefix("BUILD="))
        .unwrap_or_else(|| panic!("{} has no BUILD= line: {version:?}", path.display()));
    line.trim()
        .parse()
        .unwrap_or_else(|e| panic!("{}: BUILD={line}: {e}", path.display()))
}

/// The build of the API the binding was compiled for, from its variant's
/// name (`V<n>`).
fn binding_build(version: PdfiumApiVersion) -> u32 {
    let name = format!("{version:?}");
    name.strip_prefix('V')
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("pdfium-render binds {name}, which names no released build"))
}

#[test]
fn the_binding_the_script_and_the_fetched_library_are_one_build() {
    let script = script_build();
    let (dir, chosen) = match std::env::var_os("PDFIUM_LIB_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => (PathBuf::from(dir), true),
        None => (
            root().join(format!(
                "vendor/pdfium/{}-{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            )),
            false,
        ),
    };
    let fetched = (!chosen).then(|| fetched_build(&dir));

    let lib = Pdfium::pdfium_platform_library_name_at_path(&dir);
    let pdfium = Pdfium::new(Pdfium::bind_to_library(&lib).unwrap_or_else(|e| {
        let said = fetched.map_or_else(
            || "a library chosen by $PDFIUM_LIB_DIR".to_owned(),
            |n| format!("chromium/{n} by its VERSION"),
        );
        panic!(
            "{} ({said}) doesn't bind ({e:?}): a library that doesn't bind isn't the build \
             pdfium-render binds; scripts/fetch-pdfium.sh fetches chromium/{script}",
            lib.display()
        )
    }));
    let binding = binding_build(pdfium.bindings().version());

    assert_eq!(
        binding, script,
        "pdfium-render binds chromium/{binding}'s API (Cargo.toml's feature), and \
         scripts/fetch-pdfium.sh fetches chromium/{script}: move the two together"
    );
    if let Some(fetched) = fetched {
        assert_eq!(
            fetched,
            binding,
            "{} is chromium/{fetched} by its VERSION, and pdfium-render binds \
             chromium/{binding}: run scripts/fetch-pdfium.sh",
            dir.display()
        );
    }
}
