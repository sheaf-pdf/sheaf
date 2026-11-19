//! M18 slices 1 and 13's guard:
//! 1. `lopdf` is gone from every `.rs` file in `crates/` (including inside
//!    `sheaf_doc::pdf`).
//! 2. Every crate in the workspace reaches the `pdf-*` family crates through
//!    `sheaf_doc::pdf` (`crates/sheaf-doc/src/pdf/`). No `.rs` file outside
//!    that door may name `pdf_tokenizer`, `pdf_objects`, `pdf_filters`,
//!    `pdf_crypt`, `pdf_structure` or `pdf_content_streams` in a `use`, path
//!    or macro body.

use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::{self, Visit};
use syn::{Ident, UseTree};

const DOOR_FILE: &str = "sheaf-doc/src/pdf.rs";
const DOOR_DIR: &str = "sheaf-doc/src/pdf/";

const GONE: &[&str] = &["lopdf"];

const DOOR_ONLY: &[&str] = &[
    "pdf_tokenizer",
    "pdf_objects",
    "pdf_filters",
    "pdf_crypt",
    "pdf_structure",
    "pdf_content_streams",
];

#[test]
fn only_the_door_names_the_pdf_layer_crates() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ parent");
    let mut files = Vec::new();
    rust_files(crates, crates, &mut files);
    assert!(files.len() > 200, "walked the workspace's crates");

    let mut hits = Vec::new();
    for rel in files {
        let shown = shown(&rel);
        let in_door = shown == DOOR_FILE || shown.starts_with(DOOR_DIR);
        let full = crates.join(&rel);
        let src = std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("{shown}: {e}"));
        let file = syn::parse_file(&src).unwrap_or_else(|e| panic!("{shown}: {e}"));
        let banned: &[&str] = if in_door { GONE } else { ALL_BANNED };
        let mut scan = Scan::new(banned);
        scan.visit_file(&file);
        for (line, name) in scan.lines {
            hits.push(format!("crates/{shown}:{line} ({name})"));
        }
    }
    assert!(
        hits.is_empty(),
        "lopdf is retired, and files outside sheaf_doc::pdf must reach PDF-layer crates \
         through `sheaf_doc::pdf` (or `crate::pdf` in sheaf-doc):\n{}",
        hits.join("\n")
    );
}

const ALL_BANNED: &[&str] = &[
    "lopdf",
    "pdf_tokenizer",
    "pdf_objects",
    "pdf_filters",
    "pdf_crypt",
    "pdf_structure",
    "pdf_content_streams",
];

#[test]
fn a_probe_catches_uses_paths_and_macros_but_not_strings() {
    let src = r#"
use lopdf::Object;
use crate::pdf::Dictionary;
fn f() -> pdf_structure::Result<()> {
    let _s = "lopdf::Object and pdf_objects::Dict in a string are fine";
    let _d = pdf_objects::dict! { "K" => 1 };
    Ok(())
}
"#;
    let file = syn::parse_file(src).unwrap();
    let mut scan = Scan::new(ALL_BANNED);
    scan.visit_file(&file);
    let lines: Vec<usize> = scan.lines.into_iter().map(|(l, _)| l).collect();
    assert_eq!(lines, [2, 4, 6]);
    assert_eq!(DOOR_ONLY.len() + GONE.len(), ALL_BANNED.len());
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

struct Scan<'b> {
    banned: &'b [&'static str],
    lines: std::collections::BTreeSet<(usize, &'static str)>,
}

impl<'b> Scan<'b> {
    fn new(banned: &'b [&'static str]) -> Self {
        Self {
            banned,
            lines: std::collections::BTreeSet::new(),
        }
    }

    fn check(&mut self, ident: &Ident) {
        for &banned in self.banned {
            if ident == banned {
                self.lines.insert((ident.span().start().line, banned));
            }
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

impl<'ast> Visit<'ast> for Scan<'_> {
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
