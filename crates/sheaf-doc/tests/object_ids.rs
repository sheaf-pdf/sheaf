//! Rule 7's guard (M10.5 §2): no id is an `ObjectId`. A source names its
//! objects as its file numbers them, and so do the writers, the save
//! paths and the readers of a file's facts; everything else names an
//! object of a file through a source's types (`ObjRef`, `PageObj`,
//! `FormRef`), never lopdf's `ObjectId`.
//!
//! sheaf-doc's modules are walked from `lib.rs` as the compiler finds
//! them, each file parsed with `syn`, leaving out what only tests build:
//! an item under `#[cfg(test)]` (or the `fixtures` feature), and a module
//! declared so, its file and everything under it. A file the allowlist
//! doesn't name fails on any path or type with `ObjectId` as a segment, a
//! `use` of it (renamed too), and the name in a macro's tokens. A type
//! alias that names it, in any file, is seen through: the alias's name
//! fails as `ObjectId` does, and so does an alias of that alias.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{
    Attribute, Expr, ExprLit, Ident, ImplItem, Item, ItemType, Lit, Meta, TraitItem, UseTree,
};

/// The files and directories (ending in `/`), from `src/`, that may name
/// `ObjectId`, each with why.
const ALLOWED: &[(&str, &str)] = &[
    (
        "source/",
        "a source's objects as its file numbers them, read lazily (§3.1)",
    ),
    (
        "materialize/",
        "the emitter and the saves: objects copied, numbered and written",
    ),
    (
        "serialize/",
        "the writers, and the PDFs sheaf builds in memory",
    ),
    (
        "content/",
        "content streams and the resources they draw, read and written",
    ),
    ("forms/", "a file's form facts: its fields and widgets"),
    (
        "open/annots.rs",
        "the open's annotation facts: each one's object and what a mark holds",
    ),
    (
        "open/outline.rs",
        "the open's outline facts: each item's object, the pages it goes to",
    ),
    (
        "attachments.rs",
        "the open's attachment facts, and the `Objects` seam they read through",
    ),
    ("labels.rs", "the open's page-label facts: the number tree"),
    (
        "layers.rs",
        "the open's optional content: a layer is its group's object",
    ),
    ("portfolio.rs", "the open's portfolio facts: the collection"),
    (
        "properties.rs",
        "the Properties dialog's facts: fonts and resources, each once",
    ),
    (
        "file.rs",
        "the open's page facts: what an annotation holds, by its object",
    ),
    (
        "import.rs",
        "another tool's comment converted from its file: its appearance",
    ),
    ("realize/alloc.rs", "an overlay's object numbers"),
    (
        "realize/assemble.rs",
        "an overlay's objects, assembled into the file an engine draws",
    ),
    (
        "realize/untouched.rs",
        "whether a page draws its file's objects as they are",
    ),
    (
        "private.rs",
        "the walk of a written file for sheaf's private data",
    ),
    (
        "stamp_lib.rs",
        "stamp library files, loaded whole (proposal 29)",
    ),
    (
        "exchange/mod.rs",
        "the FDF and XFDF graphs comments are exchanged through",
    ),
    (
        "generate/saved.rs",
        "the generators a file says sheaf drew, a fact of its source: \
         its blocks' streams and the XObjects they draw",
    ),
];

/// What may never be on [`ALLOWED`]: the model, lowering and the marks.
const NEVER: &[&str] = &["model.rs", "model/", "tx/", "annot.rs"];

#[test]
fn only_a_source_s_readers_and_writers_name_an_object_id() {
    let walk = crate_walk();
    let hits = walk.hits();
    assert!(
        hits.is_empty(),
        "model-level code names lopdf's ObjectId (M10.5 §2, rule 7), or an alias of it \
         ({:?}): name a source's object by an ObjRef or a PageObj, or allowlist a reader \
         of a file's facts with its reason:\n{}",
        walk.aliases,
        hits.join("\n")
    );
}

#[test]
fn every_file_is_walked_or_only_tests_build_it() {
    let walk = crate_walk();
    let src = sheaf_doc_src();
    let mut files = Vec::new();
    rust_files(&src, &src, &mut files);
    assert!(files.len() > 100, "the walk found sheaf-doc's files");
    let walked: BTreeSet<&Path> = walk.files.iter().map(|(f, _)| f.as_path()).collect();
    let unwalked: Vec<String> = (files.iter())
        .filter(|f| !walked.contains(f.as_path()) && !walk.tests.iter().any(|t| f.starts_with(t)))
        .map(|f| shown(f))
        .collect();
    assert!(
        unwalked.is_empty(),
        "files no module declares, or the walk can't find:\n{}",
        unwalked.join("\n")
    );
}

#[test]
fn the_allowlist_is_what_names_one_and_never_the_model() {
    let walk = crate_walk();
    for &(entry, why) in ALLOWED {
        assert!(!why.is_empty(), "{entry} says why");
        let named = (walk.files.iter()).any(|(f, lines)| covers(entry, f) && !lines.is_empty());
        assert!(
            named,
            "{entry} names no ObjectId: take it off the allowlist"
        );
        for never in NEVER {
            assert!(
                !covers(entry, Path::new(never)) && !covers(never, Path::new(entry)),
                "{entry} covers {never}, which is never allowed"
            );
        }
    }
}

#[test]
fn a_probe_is_caught_outside_the_allowlist_and_not_in_tests() {
    let files = [
        (
            "lib.rs",
            r#"pub mod model;
pub mod source;
#[cfg(test)]
mod probe_tests;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixture;
pub mod inline {
    pub fn f(_: lopdf::ObjectId) {}
}
"#,
        ),
        (
            "model.rs",
            r#"use lopdf::ObjectId as Oid;
pub struct Held(Option<Oid>);
pub fn ids() -> Vec<(u32, u16)> { vec![lopdf::ObjectId::default()] }
mod helpers;
#[cfg(test)]
mod tests {
    use lopdf::ObjectId;
}
#[cfg(all(test, target_os = "linux"))]
fn linux(_: lopdf::ObjectId) {}
#[cfg(not(test))]
fn built(_: lopdf::ObjectId) {}
"#,
        ),
        ("model/helpers.rs", "pub fn g(_: ObjectId) {}\n"),
        ("source/mod.rs", "pub struct ObjRef(lopdf::ObjectId);\n"),
        ("probe_tests.rs", "use lopdf::ObjectId;\nmod deeper;\n"),
        ("probe_tests/deeper.rs", "use lopdf::ObjectId;\n"),
        ("fixture.rs", "use lopdf::ObjectId;\n"),
    ];
    let read = |path: &Path| {
        (files.iter())
            .find(|(f, _)| Path::new(f) == path)
            .map(|(_, src)| src.to_string())
    };
    let walk = Walk::of(&read);
    // Not source/ (allowed), the modules only tests build (probe_tests
    // and what it declares, the fixtures), nor model.rs's tests and its
    // Linux-only test helper; model.rs's line 2 names it renamed.
    assert_eq!(
        walk.hits(),
        [
            "lib.rs:8",
            "model.rs:1",
            "model.rs:3",
            "model.rs:12",
            "model/helpers.rs:1"
        ]
    );
    let tests: Vec<String> = walk.tests.iter().map(|t| shown(t)).collect();
    assert_eq!(
        tests,
        ["probe_tests.rs", "probe_tests", "fixture.rs", "fixture"]
    );
}

/// An alias of `ObjectId` declared where it may be named, a source's, is
/// seen through where it may not: its uses, a path through it, and an
/// alias of it. An alias only tests build isn't, nor one of something
/// else.
#[test]
fn an_alias_of_an_object_id_is_seen_through() {
    let files = [
        ("lib.rs", "pub mod model;\npub mod source;\n"),
        (
            "source/mod.rs",
            r#"pub type Id = lopdf::ObjectId;
pub type Ids = Vec<(Id, u8)>;
type Mine = ObjectId;
#[cfg(test)]
pub type Probe = lopdf::ObjectId;
pub type Count = u32;
"#,
        ),
        (
            "model.rs",
            r#"use crate::source::Id;
pub struct Held(crate::source::Ids);
pub fn f() -> Count { crate::source::Id::default().1 }
pub struct Probe;
pub type Again = Held;
pub type Renamed = Id;
fn g(_: Renamed, _: Count, _: Probe) {}
"#,
        ),
    ];
    let read = |path: &Path| {
        (files.iter())
            .find(|(f, _)| Path::new(f) == path)
            .map(|(_, src)| src.to_string())
    };
    let walk = Walk::of(&read);
    assert_eq!(
        walk.hits(),
        [
            "model.rs:1",
            "model.rs:2",
            "model.rs:3",
            "model.rs:6",
            "model.rs:7"
        ]
    );
    let aliases: Vec<&str> = walk.aliases.iter().map(String::as_str).collect();
    assert_eq!(aliases, ["Id", "Ids", "Mine", "Renamed"]);
}

/// Whether the allowlist covers `file`, a path from `src/`.
fn allowed(file: &Path) -> bool {
    ALLOWED.iter().any(|&(entry, _)| covers(entry, file))
}

/// Whether `entry`, a file or a directory ending in `/`, covers `file`.
fn covers(entry: &str, file: &Path) -> bool {
    match entry.strip_suffix('/') {
        Some(dir) => file.starts_with(dir),
        None => file == Path::new(entry),
    }
}

/// `path`, its components joined by `/` on every platform.
fn shown(path: &Path) -> String {
    let parts: Vec<_> = path.iter().map(|c| c.to_string_lossy()).collect();
    parts.join("/")
}

/// sheaf-doc's modules, walked.
fn crate_walk() -> Walk {
    let src = sheaf_doc_src();
    Walk::of(&|path| std::fs::read_to_string(src.join(path)).ok())
}

fn sheaf_doc_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// The `.rs` files under `dir`, as paths from `root`.
fn rust_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
}

/// A crate's modules, walked from `lib.rs` as the compiler finds their
/// files.
struct Walk {
    /// Each file read, by its path from `src/`, with the lines that name
    /// `ObjectId` or an alias of it.
    files: Vec<(PathBuf, Vec<usize>)>,
    /// The files, and the directories of their modules, that only tests
    /// build: never read.
    tests: Vec<PathBuf>,
    /// The type aliases that name `ObjectId`, or another of them.
    aliases: BTreeSet<String>,
}

/// A file of the walk: its path from `src/`, what it holds, and where the
/// files of the modules it declares are.
struct Parsed {
    path: PathBuf,
    file: syn::File,
    dir: PathBuf,
}

impl Walk {
    /// The crate whose files, by their paths from `src/`, `read` gives.
    fn of(read: &dyn Fn(&Path) -> Option<String>) -> Walk {
        let (mut parsed, mut tests) = (Vec::new(), Vec::new());
        module(
            read,
            Path::new("lib.rs"),
            Path::new(""),
            &mut parsed,
            &mut tests,
        );
        // `ObjectId` and its aliases, until no alias names one not yet
        // known.
        let mut names = BTreeSet::from(["ObjectId".to_string()]);
        loop {
            let scans: Vec<Scan> = (parsed.iter())
                .map(|p| Scan::of(&p.file, &p.dir, &names))
                .collect();
            let aliases: BTreeSet<String> = (scans.iter())
                .flat_map(|s| s.aliases.iter().cloned())
                .collect();
            if aliases.is_subset(&names) {
                let files = (parsed.iter().zip(scans))
                    .map(|(p, scan)| (p.path.clone(), scan.hits.into_iter().collect()))
                    .collect();
                return Walk {
                    files,
                    tests,
                    aliases,
                };
            }
            names.extend(aliases);
        }
    }

    /// Each line naming `ObjectId` in a file the allowlist doesn't cover,
    /// as `file:line`.
    fn hits(&self) -> Vec<String> {
        (self.files.iter())
            .filter(|(file, _)| !allowed(file))
            .flat_map(|(file, lines)| lines.iter().map(move |l| format!("{}:{l}", shown(file))))
            .collect()
    }
}

/// The module in file `path`, whose modules' files are in `dir`, and the
/// modules it declares, parsed into `out`; what only tests build, into
/// `tests`.
fn module(
    read: &dyn Fn(&Path) -> Option<String>,
    path: &Path,
    dir: &Path,
    out: &mut Vec<Parsed>,
    tests: &mut Vec<PathBuf>,
) {
    let src = read(path).unwrap_or_else(|| panic!("{} can't be read", path.display()));
    let file = syn::parse_file(&src).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mods = Scan::of(&file, dir, &BTreeSet::new()).mods;
    out.push(Parsed {
        path: path.to_path_buf(),
        file,
        dir: dir.to_path_buf(),
    });
    for (dir, name, only_tests) in mods {
        let (file, kids) = (dir.join(format!("{name}.rs")), dir.join(&name));
        if only_tests {
            tests.extend([file, kids]);
            continue;
        }
        let file = match read(&file) {
            Some(_) => file,
            None => kids.join("mod.rs"),
        };
        module(read, &file, &kids, out, tests);
    }
}

/// One file's code that isn't only for tests: the lines naming one of
/// `names`, the aliases that name one, and the modules it declares in
/// files of their own.
struct Scan<'a> {
    /// `ObjectId` and the aliases of it known so far.
    names: &'a BTreeSet<String>,
    /// Where the files of the modules declared here are: an inline
    /// module's under its parent's.
    dir: PathBuf,
    hits: BTreeSet<usize>,
    aliases: BTreeSet<String>,
    /// Each module declared in a file of its own: the directory its file
    /// is in, its name, and whether only tests build it.
    mods: Vec<(PathBuf, String, bool)>,
}

impl<'a> Scan<'a> {
    /// `file`, whose modules' files are in `dir`, scanned for `names`.
    fn of(file: &syn::File, dir: &Path, names: &'a BTreeSet<String>) -> Scan<'a> {
        let mut scan = Scan::new(dir, names);
        scan.visit_file(file);
        scan
    }

    fn new(dir: &Path, names: &'a BTreeSet<String>) -> Scan<'a> {
        Scan {
            names,
            dir: dir.to_path_buf(),
            hits: BTreeSet::new(),
            aliases: BTreeSet::new(),
            mods: Vec::new(),
        }
    }

    fn named(&self, ident: &Ident) -> bool {
        self.names.iter().any(|n| ident == n)
    }

    fn hit(&mut self, at: &Ident) {
        self.hits.insert(at.span().start().line);
    }

    /// A name in `tokens`, a macro's, at any depth.
    fn tokens(&mut self, tokens: TokenStream) {
        for token in tokens {
            match token {
                TokenTree::Ident(i) if self.named(&i) => self.hit(&i),
                TokenTree::Group(g) => self.tokens(g.stream()),
                _ => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Scan<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        let tests = tests_only(item_attrs(item));
        match item {
            Item::Mod(m) if m.content.is_none() => {
                self.mods
                    .push((self.dir.clone(), m.ident.to_string(), tests));
            }
            _ if tests => {}
            Item::Mod(m) => {
                self.dir.push(m.ident.to_string());
                visit::visit_item(self, item);
                self.dir.pop();
            }
            _ => visit::visit_item(self, item),
        }
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        let attrs: &[Attribute] = match item {
            ImplItem::Const(c) => &c.attrs,
            ImplItem::Fn(f) => &f.attrs,
            ImplItem::Type(t) => &t.attrs,
            ImplItem::Macro(m) => &m.attrs,
            _ => &[],
        };
        if !tests_only(attrs) {
            visit::visit_impl_item(self, item);
        }
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        let attrs: &[Attribute] = match item {
            TraitItem::Const(c) => &c.attrs,
            TraitItem::Fn(f) => &f.attrs,
            TraitItem::Type(t) => &t.attrs,
            TraitItem::Macro(m) => &m.attrs,
            _ => &[],
        };
        if !tests_only(attrs) {
            visit::visit_trait_item(self, item);
        }
    }

    fn visit_field(&mut self, f: &'ast syn::Field) {
        if !tests_only(&f.attrs) {
            visit::visit_field(self, f);
        }
    }

    fn visit_variant(&mut self, v: &'ast syn::Variant) {
        if !tests_only(&v.attrs) {
            visit::visit_variant(self, v);
        }
    }

    fn visit_arm(&mut self, a: &'ast syn::Arm) {
        if !tests_only(&a.attrs) {
            visit::visit_arm(self, a);
        }
    }

    fn visit_local(&mut self, l: &'ast syn::Local) {
        if !tests_only(&l.attrs) {
            visit::visit_local(self, l);
        }
    }

    /// An alias that names one is one: its uses count as its type's.
    fn visit_item_type(&mut self, t: &'ast ItemType) {
        let mut ty = Scan::new(&self.dir, self.names);
        ty.visit_type(&t.ty);
        if !ty.hits.is_empty() {
            self.aliases.insert(t.ident.to_string());
        }
        visit::visit_item_type(self, t);
    }

    fn visit_path(&mut self, p: &'ast syn::Path) {
        for seg in &p.segments {
            if self.named(&seg.ident) {
                self.hit(&seg.ident);
            }
        }
        visit::visit_path(self, p);
    }

    fn visit_use_tree(&mut self, tree: &'ast UseTree) {
        match tree {
            UseTree::Name(n) if self.named(&n.ident) => self.hit(&n.ident),
            UseTree::Rename(r) if self.named(&r.ident) => self.hit(&r.ident),
            _ => {}
        }
        visit::visit_use_tree(self, tree);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        self.tokens(m.tokens.clone());
        visit::visit_macro(self, m);
    }
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::ExternCrate(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::ForeignMod(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::TraitAlias(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Union(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

/// Whether `attrs` hold a `#[cfg]` only tests build under.
fn tests_only(attrs: &[Attribute]) -> bool {
    (attrs.iter())
        .filter(|a| a.path().is_ident("cfg"))
        .any(|a| a.parse_args::<Meta>().is_ok_and(|pred| for_tests(&pred)))
}

/// Whether `pred`, a `cfg` predicate, holds only for tests: `test`, the
/// `fixtures` feature (what other crates' tests build), `any` of those, or
/// `all` of something with one of them.
fn for_tests(pred: &Meta) -> bool {
    match pred {
        Meta::Path(p) => p.is_ident("test"),
        Meta::NameValue(nv) => {
            nv.path.is_ident("feature")
                && matches!(&nv.value, Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) if s.value() == "fixtures")
        }
        Meta::List(list) => {
            let parse = Punctuated::<Meta, syn::Token![,]>::parse_terminated;
            let Ok(args) = list.parse_args_with(parse) else {
                return false;
            };
            if list.path.is_ident("any") {
                !args.is_empty() && args.iter().all(for_tests)
            } else {
                list.path.is_ident("all") && args.iter().any(for_tests)
            }
        }
    }
}
