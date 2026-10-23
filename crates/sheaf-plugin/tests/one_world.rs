//! sheaf serves one plugin world (M10.5 §14.3): `wit/` holds its one
//! package, the host binds it once, and the plugin API generates it once.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// How many times `dir`'s Rust sources, read whole, say `needle`.
fn said(dir: &Path, needle: &str) -> usize {
    let mut count = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            count += said(&path, needle);
        } else if path.extension().is_some_and(|e| e == "rs") {
            count += std::fs::read_to_string(&path)
                .unwrap()
                .matches(needle)
                .count();
        }
    }
    count
}

#[test]
fn one_world_is_bound_once() {
    let wit: Vec<String> = std::fs::read_dir(root().join("wit"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(wit, ["sheaf.wit"]);
    let host = root().join("crates/sheaf-plugin/src");
    assert_eq!(said(&host, "bindgen!("), 1, "the host's binding");
    let api = root().join("crates/sheaf-plugin-api/src");
    assert_eq!(said(&api, "generate!("), 1, "the plugin API's");
}
