//! M18 slice 1's guard: every crate in the workspace reaches the PDF
//! layer through `sheaf_doc::pdf` (`crates/sheaf-doc/src/pdf.rs`, or
//! `crates/sheaf-doc/src/pdf/`). No other `.rs` file under `crates/`
//! may name `lopdf` in a `use`, path or macro body.

use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::{self, Visit};
use syn::{Ident, UseTree};

const DOOR_FILE: &str = "sheaf-doc/src/pdf.rs";
const DOOR_DIR: &str = "sheaf-doc/src/pdf/";

#[test]
fn only_the_door_names_lopdf() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ parent");
    let mut files = Vec::new();
    rust_files(crates, crates, &mut files);
    assert!(files.len() > 200, "walked the workspace's crates");

    let mut hits = Vec::new();
    for rel in files {
        let shown = shown(&rel);
        if shown == DOOR_FILE || shown.starts_with(DOOR_DIR) {
            continue;
        }
        let full = crates.join(&rel);
        let src = std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("{shown}: {e}"));
        let file = syn::parse_file(&src).unwrap_or_else(|e| panic!("{shown}: {e}"));
        let mut scan = Scan::default();
        scan.visit_file(&file);
        for line in scan.lines {
            hits.push(format!("crates/{shown}:{line}"));
        }
    }
    assert!(
        hits.is_empty(),
        "files outside sheaf_doc::pdf name `lopdf` directly; reach it through \
         `sheaf_doc::pdf` (or `crate::pdf` in sheaf-doc):\n{}",
        hits.join("\n")
    );
}

#[test]
fn a_probe_catches_uses_paths_and_macros_but_not_strings() {
    let src = r#"
use lopdf::Object;
use crate::pdf::Dictionary;
fn f() -> lopdf::Result<()> {
    let _s = "lopdf::Object in a string is fine";
    let _d = lopdf::dictionary! { "K" => 1 };
    Ok(())
}
"#;
    let file = syn::parse_file(src).unwrap();
    let mut scan = Scan::default();
    scan.visit_file(&file);
    let lines: Vec<usize> = scan.lines.into_iter().collect();
    assert_eq!(lines, [2, 4, 6]);
}

fn rust_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_files(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
}

fn shown(path: &Path) -> String {
    let parts: Vec<_> = path.iter().map(|c| c.to_string_lossy()).collect();
    parts.join("/")
}

#[derive(Default)]
struct Scan {
    lines: std::collections::BTreeSet<usize>,
}

impl Scan {
    fn check(&mut self, ident: &Ident) {
        if ident == "lopdf" {
            self.lines.insert(ident.span().start().line);
        }
    }

    fn tokens(&mut self, stream: TokenStream) {
        for tt in stream {
            match tt {
                TokenTree::Ident(i) => self.check(&i),
                TokenTree::Group(g) => self.tokens(g.stream()),
                _ => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Scan {
    fn visit_path(&mut self, p: &'ast syn::Path) {
        if let Some(first) = p.segments.first() {
            self.check(&first.ident);
        }
        visit::visit_path(self, p);
    }

    fn visit_use_tree(&mut self, tree: &'ast UseTree) {
        match tree {
            UseTree::Path(p) => self.check(&p.ident),
            UseTree::Name(n) => self.check(&n.ident),
            UseTree::Rename(r) => self.check(&r.ident),
            _ => {}
        }
        visit::visit_use_tree(self, tree);
    }

    fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
        self.check(&item.ident);
        visit::visit_item_extern_crate(self, item);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        self.tokens(m.tokens.clone());
        visit::visit_macro(self, m);
    }
}
