//! The predicate language: every predicate in the model parses and prints
//! back as written, with a census of its functions; and each supported
//! function means what INTERNAL_GRAMMAR.md says, on the model's own cells
//! where it has them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use lopdf::{Dictionary, Object, ObjectId, Stream, StringFormat, dictionary};

use super::grammar::{split, version};
use super::predicate::{
    Expr, SUPPORTED, gate, holds, holds_fully, parse, reference, required, since,
};
use super::*;
use crate::materialize::emit::PdfVersion;

/// A version the model names.
fn v(text: &str) -> PdfVersion {
    version(text).unwrap()
}

/// The vendored tables' files, by name.
fn files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(DIR)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "tsv"))
        .collect();
    files.sort();
    files
}

/// Every predicate in the model, read from the files as they are: where
/// it is (table, line and column) and its text. A cell is split into its
/// types' groups and those into their items (INTERNAL_GRAMMAR.md, "TSV
/// Data Fields"); Key holds none, and Note is prose.
fn predicates() -> Vec<(String, String)> {
    let mut found = Vec::new();
    for path in files() {
        let name = path.file_stem().unwrap().to_str().unwrap().to_string();
        let text = std::fs::read_to_string(&path).unwrap();
        for (i, line) in text.lines().enumerate().skip(1) {
            let cells: Vec<&str> = line.split('\t').collect();
            for (column, cell) in grammar::HEADER.iter().zip(&cells).take(11).skip(1) {
                for group in split(cell, ';').unwrap() {
                    let inner = group.strip_prefix('[').and_then(|g| g.strip_suffix(']'));
                    for item in split(inner.unwrap_or(group), ',').unwrap() {
                        if item.contains("fn:") {
                            found.push((format!("{name}:{} {column}", i + 1), item.to_string()));
                        }
                    }
                }
            }
        }
    }
    found
}

/// Every call in `e`, by function, counted into `census`.
fn calls(e: &Expr, census: &mut BTreeMap<String, usize>) {
    match e {
        Expr::Call(name, args) => {
            *census.entry(name.clone()).or_default() += 1;
            args.iter().for_each(|a| calls(a, census));
        }
        Expr::Group(inner) => calls(inner, census),
        Expr::And(items) | Expr::Or(items) => items.iter().for_each(|i| calls(i, census)),
        Expr::Cmp(_, a, b) => {
            calls(a, census);
            calls(b, census);
        }
        Expr::Arith(first, rest) => {
            calls(first, census);
            rest.iter().for_each(|(_, e)| calls(e, census));
        }
        Expr::Path(_)
        | Expr::Int(_)
        | Expr::Real(_)
        | Expr::Str(_)
        | Expr::Bool(_)
        | Expr::Null => {}
    }
}

/// How deeply `text` nests its brackets.
fn nesting(text: &str) -> usize {
    let (mut depth, mut deepest, mut quoted) = (0usize, 0, false);
    for c in text.chars() {
        match c {
            '\'' => quoted = !quoted,
            '(' if !quoted => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            ')' if !quoted => depth -= 1,
            _ => {}
        }
    }
    deepest
}

#[test]
fn every_predicate_in_the_model_parses() {
    let all = predicates();
    let mut census = BTreeMap::new();
    let mut failed = Vec::new();
    for (at, text) in &all {
        match parse(text) {
            Ok(e) => {
                assert_eq!(e.to_string(), *text, "{at}: it prints back as written");
                calls(&e, &mut census);
            }
            Err(e) => failed.push(format!("{at}: {e:#}")),
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {} predicates don't parse:\n{}",
        failed.len(),
        all.len(),
        failed.join("\n")
    );
    // Every `fn:` in the model's cells is a call in one of them.
    let mut written = 0;
    for path in files() {
        let text = std::fs::read_to_string(path).unwrap();
        for line in text.lines().skip(1) {
            let cells = line.split('\t').take(11).skip(1);
            written += cells.map(|cell| cell.matches("fn:").count()).sum::<usize>();
        }
    }
    assert_eq!(census.values().sum::<usize>(), written);
    for name in SUPPORTED {
        assert!(
            census.contains_key(name),
            "{name} is supported but the model has none"
        );
    }

    let deepest = all.iter().map(|(_, text)| nesting(text)).max().unwrap();
    println!(
        "Arlington predicates: {} in the model's cells, {written} calls of {} functions, nested at most {deepest} deep",
        all.len(),
        census.len()
    );
    let mut by_count: Vec<_> = census.iter().collect();
    by_count.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (name, n) in by_count {
        let supported = if SUPPORTED.contains(&name.as_str()) {
            "supported"
        } else {
            "unsupported: unknown, counted"
        };
        println!("  {n:>5}  {name:<34} {supported}");
    }
}

#[test]
fn every_since_required_and_indirect_reference_cell_has_a_form_its_reader_reads() {
    let mut since_in = BTreeMap::new();
    let mut extensions = 0;
    for table in grammar().tables() {
        for row in &table.rows {
            let at = format!("{}:{}", table.name, row.line);
            match since(&row.since_version) {
                Since::In(v) => *since_in.entry(v).or_insert(0) += 1,
                Since::Extension(_) => extensions += 1,
                Since::Unknown(cell) => panic!("{at}: SinceVersion {cell:?} doesn't read"),
            }
            if !matches!(row.required.as_str(), "TRUE" | "FALSE") {
                let call = parse(&row.required).unwrap();
                assert!(
                    matches!(&call, Expr::Call(f, args) if f == "fn:IsRequired" && args.len() == 1),
                    "{at}: Required {call}"
                );
            }
            for cell in &row.indirect_reference {
                if !matches!(cell.as_str(), "TRUE" | "FALSE") {
                    let call = parse(cell).unwrap();
                    let Expr::Call(f, args) = &call else {
                        panic!("{at}: IndirectReference {call}");
                    };
                    assert!(
                        matches!(f.as_str(), "fn:MustBeDirect" | "fn:MustBeIndirect")
                            && args.len() <= 1,
                        "{at}: IndirectReference {call}"
                    );
                }
            }
        }
    }
    let by_version: Vec<String> = (since_in.iter())
        .map(|(v, n)| format!("{}.{} {n}", v.major, v.minor))
        .collect();
    println!(
        "SinceVersion: {extensions} keys an extension's; by version, {}",
        by_version.join(", ")
    );
}

/// The trailer and objects of a file, for predicates to read.
struct File {
    trailer: Dictionary,
    objects: BTreeMap<ObjectId, Object>,
}

impl File {
    fn new(trailer: Dictionary, objects: impl IntoIterator<Item = (u32, Object)>) -> File {
        let objects = objects.into_iter().map(|(n, o)| ((n, 0), o)).collect();
        File { trailer, objects }
    }

    /// What `f` says of the row at `at` in `container`, reached through
    /// `parents`, in this file at `version`.
    fn eval<T>(
        &self,
        container: &Object,
        at: At,
        parents: &[&Object],
        version: &str,
        f: impl FnOnce(&Cx) -> T,
    ) -> T {
        let resolve = |id: ObjectId| self.objects.get(&id);
        f(&Cx {
            container,
            at,
            parents,
            trailer: &self.trailer,
            resolve: &resolve,
            version: v(version),
        })
    }
}

/// A file with nothing but a catalog, object 1.
fn bare() -> File {
    let catalog = Object::Dictionary(dictionary! { "Type" => "Catalog" });
    File::new(
        dictionary! { "Root" => Object::Reference((1, 0)) },
        [(1, catalog)],
    )
}

/// The model's row `key` of `table`.
fn row(table: &str, key: &[u8]) -> &'static Row {
    grammar().table(table).unwrap().row(At::Key(key)).unwrap()
}

/// `text` held of `container` at `version` in `file`.
fn holds_in(file: &File, container: &Object, version: &str, text: &str) -> Truth {
    file.eval(container, At::Key(b"X"), &[], version, |cx| holds(text, cx))
}

/// `cell` as Required of `container` at `version` in `file`.
fn required_in(file: &File, container: &Object, version: &str, cell: &str) -> Truth {
    file.eval(container, At::Key(b"X"), &[], version, |cx| {
        required(cell, cx)
    })
}

fn unknown(why: &str) -> Truth {
    Truth::Unknown(why.to_string())
}

#[test]
fn is_present_reads_keys_paths_from_the_trailer_and_any_entry() {
    let catalog =
        Object::Dictionary(dictionary! { "Type" => "Catalog", "PieceInfo" => dictionary! {} });
    let file = File::new(
        dictionary! { "Root" => Object::Reference((1, 0)) },
        [(1, catalog)],
    );
    let here = Object::Dictionary(dictionary! {
        "Null" => Object::Null,
        "Gone" => Object::Reference((9, 0)),
        "Kept" => 1,
    });
    let present = |text: &str| holds_in(&file, &here, "1.7", text);
    assert_eq!(
        present("fn:IsPresent(trailer::Catalog::PieceInfo)"),
        Truth::True
    );
    assert_eq!(
        present("fn:IsPresent(trailer::Root::PieceInfo)"),
        Truth::True,
        "Root is Catalog"
    );
    assert_eq!(
        present("fn:IsPresent(trailer::Info::ModDate)"),
        Truth::False
    );
    assert_eq!(present("fn:IsPresent(Kept)"), Truth::True);
    assert_eq!(
        present("fn:IsPresent(Null)"),
        Truth::False,
        "null is absent"
    );
    assert_eq!(
        present("fn:IsPresent(Gone)"),
        Truth::False,
        "a reference to no object is null"
    );
    assert_eq!(present("fn:Not(fn:IsPresent(Gone))"), Truth::True);

    // An annotation's color has a third element if it has a second: an
    // array's element by its index.
    let second = &(grammar().table("ArrayOf_4NumbersColorAnnotation").unwrap())
        .row(At::Index(1))
        .unwrap()
        .special_case[0];
    for (color, met) in [(vec![0, 1], Truth::False), (vec![0, 1, 0], Truth::True)] {
        let color = Object::Array(color.into_iter().map(Object::Integer).collect());
        let is = file.eval(&color, At::Index(1), &[], "1.7", |cx| holds(second, cx));
        assert_eq!(is, met, "{second}: {color:?}");
    }

    // An annotation's AS is required when its appearances are
    // dictionaries of states, not when they're one stream each.
    let appearance =
        || Object::Stream(Stream::new(dictionary! { "Subtype" => "Form" }, Vec::new()));
    let annot = |n: Object| Object::Dictionary(dictionary! { "AP" => dictionary! { "N" => n } });
    let states = annot(Object::Dictionary(
        dictionary! { "Off" => appearance(), "On" => appearance() },
    ));
    let one = annot(appearance());
    let empty = annot(Object::Dictionary(dictionary! { "Off" => Object::Null }));
    let as_ = &row("AnnotSquare", b"AS").required;
    assert_eq!(required_in(&file, &states, "1.7", as_), Truth::True);
    assert_eq!(required_in(&file, &one, "1.7", as_), Truth::False);
    assert_eq!(
        required_in(&file, &empty, "1.7", as_),
        Truth::False,
        "a null entry is none"
    );

    // The trailer's Info, from 1.4 with a catalog's PieceInfo: its
    // ModDate. Without PieceInfo, or before 1.4, it asks nothing.
    let info = &row("FileTrailer", b"Info").special_case[0];
    let with = |info_dict: Dictionary, catalog: Dictionary| {
        File::new(
            dictionary! { "Root" => Object::Reference((1, 0)), "Info" => Object::Reference((2, 0)) },
            [
                (1, Object::Dictionary(catalog)),
                (2, Object::Dictionary(info_dict)),
            ],
        )
    };
    let pieces = || dictionary! { "PieceInfo" => dictionary! {} };
    let dated = || dictionary! { "ModDate" => Object::string_literal("D:20261024") };
    for (file, version, met) in [
        (with(dated(), pieces()), "1.4", Truth::True),
        (with(dictionary! {}, pieces()), "1.4", Truth::False),
        (with(dictionary! {}, pieces()), "1.3", Truth::True),
        (with(dictionary! {}, dictionary! {}), "1.7", Truth::True),
    ] {
        let trailer = Object::Dictionary(file.trailer.clone());
        assert_eq!(
            holds_in(&file, &trailer, version, info),
            met,
            "at {version}"
        );
    }
}

#[test]
fn the_trailers_info_is_required_from_1_4_when_the_catalog_has_piece_info() {
    let info = &row("FileTrailer", b"Info").required;
    let pieces =
        Object::Dictionary(dictionary! { "Type" => "Catalog", "PieceInfo" => dictionary! {} });
    let file = File::new(
        dictionary! { "Root" => Object::Reference((1, 0)) },
        [(1, pieces)],
    );
    let trailer = Object::Dictionary(file.trailer.clone());
    assert_eq!(required_in(&file, &trailer, "1.3", info), Truth::False);
    assert_eq!(required_in(&file, &trailer, "1.4", info), Truth::True);
    let bare = bare();
    let trailer = Object::Dictionary(bare.trailer.clone());
    assert_eq!(required_in(&bare, &trailer, "1.4", info), Truth::False);
}

#[test]
fn an_annotations_appearance_is_required_from_2_0_unless_its_rect_is_a_point() {
    let ap = &row("AnnotSquare", b"AP").required;
    let file = bare();
    let square = |rect: [i64; 4]| {
        let rect: Vec<Object> = rect.into_iter().map(Object::Integer).collect();
        Object::Dictionary(dictionary! { "Subtype" => "Square", "Rect" => rect })
    };
    let wide = square([0, 0, 100, 50]);
    assert_eq!(required_in(&file, &wide, "1.7", ap), Truth::False);
    assert_eq!(required_in(&file, &wide, "2.0", ap), Truth::True);
    let point = square([5, 5, 5, 5]);
    assert_eq!(required_in(&file, &point, "2.0", ap), Truth::False);
    let zero_width = square([5, 0, 5, 10]);
    assert_eq!(
        required_in(&file, &zero_width, "2.0", ap),
        Truth::True,
        "it has a height"
    );
    let turned = square([100, 50, 0, 0]);
    assert_eq!(
        required_in(&file, &turned, "2.0", ap),
        Truth::True,
        "any two opposite corners"
    );
    let unboxed = Object::Dictionary(dictionary! { "Subtype" => "Square" });
    assert_eq!(required_in(&file, &unboxed, "2.0", ap), unknown("Rect"));
}

#[test]
fn a_pages_parent_is_required_unless_it_is_a_template() {
    let parent = row("PageObject", b"Parent");
    let file = bare();
    let page = |ty: &str| Object::Dictionary(dictionary! { "Type" => ty });
    assert_eq!(
        required_in(&file, &page("Page"), "1.7", &parent.required),
        Truth::True
    );
    assert_eq!(
        required_in(&file, &page("Template"), "1.7", &parent.required),
        Truth::False
    );
    let untyped = Object::Dictionary(dictionary! {});
    assert_eq!(
        required_in(&file, &untyped, "1.7", &parent.required),
        unknown("@Type")
    );
    // And a template's shall not be present: fn:Not(fn:IsPresent(cond)).
    let special = &parent.special_case[0];
    assert_eq!(holds_in(&file, &page("Page"), "1.7", special), Truth::True);
    assert_eq!(
        holds_in(&file, &page("Template"), "1.7", special),
        Truth::False
    );
}

#[test]
fn version_predicates_hold_at_their_edges() {
    let file = bare();
    let x = Object::Dictionary(dictionary! { "X" => 1 });
    let req = |version: &str, cell: &str| required_in(&file, &x, version, cell);
    for (version, is) in [("1.6", false), ("1.7", true), ("2.0", false)] {
        assert_eq!(
            req(version, "fn:IsRequired(fn:IsPDFVersion(1.7))") == Truth::True,
            is,
            "{version}"
        );
    }
    for (version, is) in [("1.2", true), ("1.3", false)] {
        assert_eq!(
            req(version, "fn:IsRequired(fn:BeforeVersion(1.3))") == Truth::True,
            is,
            "{version}"
        );
    }
    for (version, is) in [("1.4", false), ("1.5", true)] {
        assert_eq!(
            req(version, "fn:IsRequired(fn:SinceVersion(1.5))") == Truth::True,
            is,
            "{version}"
        );
    }
    // A statement where it doesn't apply asks nothing: no requirement, and
    // nothing to meet.
    let statement = "fn:IsRequired(fn:SinceVersion(2.0,fn:IsPresent(X)))";
    assert_eq!(req("1.7", statement), Truth::False);
    assert_eq!(req("2.0", statement), Truth::True);
    let before = "fn:BeforeVersion(2.0,fn:Not(fn:IsPresent(X)))";
    assert_eq!(holds_in(&file, &x, "1.7", before), Truth::False);
    assert_eq!(holds_in(&file, &x, "2.0", before), Truth::True);
    // An annotation's F: the bits each version leaves clear, none before
    // 1.3. ToggleNoView, bit 9, came in 1.5.
    let f = &row("AnnotSquare", b"F").special_case[0];
    let met = |version: &str, flags: i64| {
        let annot = Object::Dictionary(dictionary! { "F" => flags });
        file.eval(&annot, At::Key(b"F"), &[], version, |cx| holds(f, cx))
    };
    for (version, toggle) in [
        ("1.2", true),
        ("1.3", false),
        ("1.4", false),
        ("1.5", true),
        ("2.0", true),
    ] {
        assert_eq!(met(version, 4), Truth::True, "{version}: Print");
        let toggle = if toggle { Truth::True } else { Truth::False };
        assert_eq!(met(version, 1 << 8), toggle, "{version}: ToggleNoView");
    }
}

#[test]
fn an_extension_and_then_a_version_is_since_the_version() {
    assert_eq!(since("1.5"), Since::In(v("1.5")));
    assert_eq!(
        since(&row("CollectionSplit", b"Direction").since_version),
        Since::In(v("2.0"))
    );
    assert_eq!(
        since("fn:Eval(fn:Extension(X,1.7) || 2.0)"),
        Since::In(v("2.0"))
    );
    assert_eq!(
        since(&row("FileTrailer", b"AuthCode").since_version),
        Since::Extension("ISO_TS_32004".to_string())
    );
    assert_eq!(
        since("fn:Extension(AAPL)"),
        Since::Extension("AAPL".to_string())
    );
    assert_eq!(
        since("2.1"),
        Since::Unknown("2.1".to_string()),
        "not a version the model has"
    );
    // No extension is allowed: false, and a statement under one asks
    // nothing.
    let file = bare();
    let x = Object::Dictionary(dictionary! {});
    assert_eq!(
        required_in(&file, &x, "2.0", "fn:IsRequired(fn:Extension(PDF_VT2))"),
        Truth::False
    );
    let under = "fn:Extension(ETSI_PAdES,fn:IsPresent(Cert))";
    assert_eq!(holds_in(&file, &x, "2.0", under), Truth::True);
}

#[test]
fn deprecated_allows_what_it_marks() {
    let values = &row("EncryptionStandard", b"V").possible_values[0];
    let two = values.iter().find(|w| w.item == "2").unwrap();
    assert_eq!(gate(&two.wraps, v("1.3")), Gate::Excluded, "since 1.4");
    assert_eq!(gate(&two.wraps, v("1.7")), Gate::Allowed);
    assert_eq!(
        gate(&two.wraps, v("2.0")),
        Gate::AllowedDeprecated(v("2.0"))
    );
    let zero = values.iter().find(|w| w.item == "0").unwrap();
    assert_eq!(gate(&zero.wraps, v("1.0")), Gate::Allowed);
    assert_eq!(
        gate(&zero.wraps, v("1.7")),
        Gate::AllowedDeprecated(v("1.1")),
        "deprecated in 1.1"
    );

    let wrap = |func, arg: &str| Wrap {
        func,
        arg: arg.to_string(),
    };
    let from = [wrap(WrapFn::SinceVersion, "1.4")];
    assert_eq!(gate(&from, v("1.3")), Gate::Excluded);
    assert_eq!(gate(&from, v("1.4")), Gate::Allowed);
    let before = [wrap(WrapFn::BeforeVersion, "1.3")];
    assert_eq!(gate(&before, v("1.2")), Gate::Allowed);
    assert_eq!(gate(&before, v("1.3")), Gate::Excluded);
    let only = [wrap(WrapFn::IsPDFVersion, "1.2")];
    assert_eq!(gate(&only, v("1.1")), Gate::Excluded);
    assert_eq!(gate(&only, v("1.2")), Gate::Allowed);
    assert_eq!(gate(&only, v("1.3")), Gate::Excluded);
    assert_eq!(gate(&[], v("1.0")), Gate::Allowed);
    let twice = [
        wrap(WrapFn::Deprecated, "1.7"),
        wrap(WrapFn::Deprecated, "1.4"),
    ];
    assert_eq!(gate(&twice, v("1.5")), Gate::AllowedDeprecated(v("1.4")));
    assert_eq!(gate(&twice, v("2.0")), Gate::AllowedDeprecated(v("1.4")));
    // An extension's link, at its version or not, is no version's.
    let annots = grammar()
        .table("ArrayOfAnnots")
        .unwrap()
        .row(At::Index(0))
        .unwrap();
    let projection = annots.link[0]
        .iter()
        .find(|l| l.item == "AnnotProjection")
        .unwrap();
    assert_eq!(gate(&projection.wraps, v("1.7")), Gate::Excluded);
}

#[test]
fn the_trailers_id_must_be_direct_when_the_file_is_encrypted() {
    let id = &row("FileTrailer", b"ID").indirect_reference[0];
    let file = bare();
    let reads = |trailer: Dictionary, version: &str, cell: &str| {
        let trailer = Object::Dictionary(trailer);
        file.eval(&trailer, At::Key(b"ID"), &[], version, |cx| {
            reference(cell, cx)
        })
    };
    let encrypted =
        || dictionary! { "Root" => Object::Reference((1, 0)), "Encrypt" => dictionary! {} };
    assert_eq!(reads(encrypted(), "1.7", id), Reference::Direct);
    assert_eq!(reads(file.trailer.clone(), "1.7", id), Reference::Either);
    let info = &row("FileTrailer", b"Info").indirect_reference[0];
    assert_eq!(
        reads(file.trailer.clone(), "1.7", info),
        Reference::Indirect
    );
    assert_eq!(reads(file.trailer.clone(), "2.0", info), Reference::Either);
    let size = &row("FileTrailer", b"Size").indirect_reference[0];
    assert_eq!(reads(file.trailer.clone(), "1.7", size), Reference::Direct);
    let root = &row("FileTrailer", b"Root").indirect_reference[0];
    assert_eq!(
        reads(file.trailer.clone(), "1.7", root),
        Reference::Indirect
    );
    let encrypt = &row("FileTrailer", b"Encrypt").indirect_reference[0];
    assert_eq!(
        reads(file.trailer.clone(), "1.7", encrypt),
        Reference::Either
    );
}

#[test]
fn unknown_goes_through_and_and_or_until_something_decides() {
    let file = bare();
    let x = |n: i64| Object::Dictionary(dictionary! { "X" => n });
    let is = |n, text: &str| holds_in(&file, &x(n), "1.7", text);
    assert_eq!(
        is(2, "fn:Eval(fn:IsEncryptedWrapper() && (@X==1))"),
        Truth::False
    );
    assert_eq!(
        is(2, "fn:Eval((@X==1) && fn:IsEncryptedWrapper())"),
        Truth::False
    );
    assert_eq!(
        is(1, "fn:Eval(fn:IsEncryptedWrapper() && (@X==1))"),
        unknown("fn:IsEncryptedWrapper")
    );
    assert_eq!(
        is(1, "fn:Eval(fn:IsEncryptedWrapper() || (@X==1))"),
        Truth::True
    );
    assert_eq!(
        is(2, "fn:Eval(fn:IsEncryptedWrapper() || (@X==1))"),
        unknown("fn:IsEncryptedWrapper")
    );
    assert_eq!(
        is(1, "fn:Eval(fn:IsMeaningful() && fn:IsEncryptedWrapper())"),
        unknown("fn:IsMeaningful"),
        "the first"
    );
    assert_eq!(
        is(1, "fn:Not(fn:IsEncryptedWrapper())"),
        unknown("fn:IsEncryptedWrapper")
    );
}

#[test]
fn a_rule_holds_fully_only_when_every_function_in_it_is_supported() {
    let file = bare();
    let x = |n: i64| Object::Dictionary(dictionary! { "X" => n });
    let fully =
        |n, text: &str| file.eval(&x(n), At::Key(b"X"), &[], "1.7", |cx| holds_fully(text, cx));
    let decided = "fn:Eval(fn:IsEncryptedWrapper() || (@X==1))";
    assert_eq!(holds_in(&file, &x(1), "1.7", decided), Truth::True);
    assert_eq!(fully(1, decided), unknown("fn:IsEncryptedWrapper"));
    assert_eq!(
        fully(1, "fn:Eval((@X==1) && fn:Not(fn:IsMeaningful(X)))"),
        unknown("fn:IsMeaningful"),
        "however deep"
    );
    assert_eq!(fully(1, "fn:Eval((@X>=0) && (@X<=1))"), Truth::True);
    assert_eq!(fully(2, "fn:Eval((@X>=0) && (@X<=1))"), Truth::False);
    assert_eq!(fully(2, "fn:Eval((@X mod 2)==0)"), unknown("mod"));
}

#[test]
fn an_unsupported_function_and_arithmetic_are_unknown() {
    let file = bare();
    let x = Object::Dictionary(dictionary! { "A" => 1, "B" => 2 });
    let is = |text: &str| holds_in(&file, &x, "1.7", text);
    assert_eq!(
        is("fn:Eval(fn:IsEncryptedWrapper())"),
        unknown("fn:IsEncryptedWrapper")
    );
    assert_eq!(is("fn:Eval((@A+@B)==3)"), unknown("+"));
    assert_eq!(is("fn:Eval((@A - @B)<0)"), unknown("-"));
    assert_eq!(is("fn:Eval((fn:ArrayLength(A) mod 2)==0)"), unknown("mod"));
}

#[test]
fn comparisons_read_names_numbers_strings_and_booleans() {
    let file = bare();
    let utf16: Vec<u8> = [0xFE, 0xFF]
        .into_iter()
        .chain("Marked".encode_utf16().flat_map(u16::to_be_bytes))
        .collect();
    let x = Object::Dictionary(dictionary! {
        "Type" => "Page",
        "V" => Object::Real(4.0),
        "CA" => 1,
        "StateModel" => Object::String(utf16, StringFormat::Hexadecimal),
        "ImageMask" => true,
        "Matrix" => vec![Object::Integer(1), Object::Integer(0)],
    });
    let is = |text: &str| holds_in(&file, &x, "1.7", text);
    assert_eq!(is("fn:Eval(@Type==Page)"), Truth::True);
    assert_eq!(is("fn:Eval(@Type!=Page)"), Truth::False);
    assert_eq!(
        is("fn:Eval(@V==4)"),
        Truth::True,
        "an integer stands for a number"
    );
    assert_eq!(is("fn:Eval((@CA>=0.0) && (@CA<=1.0))"), Truth::True);
    assert_eq!(is("fn:Eval(@CA<1)"), Truth::False);
    assert_eq!(
        is("fn:Eval(@StateModel=='Marked')"),
        Truth::True,
        "a text string's text"
    );
    assert_eq!(is("fn:Eval(@ImageMask==true)"), Truth::True);
    assert_eq!(
        is("fn:Eval(@Type==4)"),
        unknown("@Type==4"),
        "a name isn't a number"
    );
    assert_eq!(
        is("fn:Eval(@Type<Page)"),
        unknown("@Type<Page"),
        "names aren't in order"
    );
    assert_eq!(
        is("fn:Eval(@Matrix==1)"),
        unknown("@Matrix"),
        "an array isn't compared"
    );
    assert_eq!(is("fn:Eval(fn:ArrayLength(Matrix)==2)"), Truth::True);
    assert_eq!(
        is("fn:Eval(fn:ArrayLength(Nonesuch)==2)"),
        unknown("Nonesuch")
    );

    // A key a parent has: a VRI's CRL is required when the DSS's CRLs has
    // any.
    let crl = &row("VRI", b"CRL").required;
    let dss = |n: usize| Object::Dictionary(dictionary! { "CRLs" => vec![Object::Null; n] });
    let vri = Object::Dictionary(dictionary! {});
    let required_under =
        |dss: &Object| file.eval(&vri, At::Key(b"CRL"), &[dss], "1.7", |cx| required(crl, cx));
    assert_eq!(required_under(&dss(2)), Truth::True);
    assert_eq!(required_under(&dss(0)), Truth::False);
    assert_eq!(
        file.eval(&vri, At::Key(b"CRL"), &[], "1.7", |cx| required(crl, cx)),
        unknown("parent::CRLs")
    );

    // A soft mask with a Matte is the size of its image.
    let width = &row("XObjectImageSoftMask", b"Width").special_case[0];
    let image = Object::Dictionary(dictionary! { "Width" => 10 });
    let mask = |w: i64, matte: bool| {
        let mut mask = dictionary! { "Width" => w };
        if matte {
            mask.set("Matte", vec![Object::Integer(0)]);
        }
        Object::Stream(Stream::new(mask, Vec::new()))
    };
    let met = |mask: &Object| {
        file.eval(mask, At::Key(b"Width"), &[&image], "1.7", |cx| {
            holds(width, cx)
        })
    };
    assert_eq!(met(&mask(10, true)), Truth::True);
    assert_eq!(met(&mask(11, true)), Truth::False);
    assert_eq!(
        met(&mask(11, false)),
        Truth::True,
        "without a Matte it asks nothing"
    );
}

#[test]
fn a_wildcard_first_is_the_rows_own_value() {
    let file = bare();
    let array = Object::Array(vec![
        Object::Integer(1),
        Object::Integer(2),
        Object::Integer(-1),
    ]);
    let at =
        |i: usize, text: &str| file.eval(&array, At::Index(i), &[], "1.7", |cx| holds(text, cx));
    assert_eq!(at(1, "fn:Eval(@*>0)"), Truth::True);
    assert_eq!(at(2, "fn:Eval(@*>0)"), Truth::False);
    assert_eq!(at(2, "fn:Eval(@1*>=0)"), Truth::False);
    assert_eq!(at(0, "fn:Eval(@0<=@1)"), Truth::True, "indices");
    let urls = Object::Array(vec![Object::Dictionary(dictionary! { "FS" => "URL" })]);
    let url = file.eval(&urls, At::Index(0), &[], "2.0", |cx| {
        holds("fn:Eval(fn:IsPresent(*::FS) && (*::@FS==URL))", cx)
    });
    assert_eq!(url, Truth::True);
    // `*` past the start is any entry, which only presence reads.
    assert_eq!(at(0, "fn:Eval(A::@*==1)"), unknown("A::@*"));
}

#[test]
fn a_predicate_that_isnt_one_doesnt_parse() {
    let nested = |n: usize| format!("{}fn:IsPresent(X){}", "fn:Not(".repeat(n), ")".repeat(n));
    assert!(parse(&nested(20)).is_ok());
    let Err(e) = parse(&nested(40)) else {
        panic!("40 deep parsed");
    };
    assert!(format!("{e:#}").contains("nested deeper"), "{e:#}");
    for bad in [
        "(@A==1) && (@B==2) || (@C==3)",
        "fn:Not(a,b,c)",
        "fn:eval(@A==1)",
        "@A=='open",
        "@A==''",
        "@A == 1",
        "fn:IsPresent(A)x",
        "fn:IsPresent(A::parent::B)",
        "fn:IsPresent(trailer)",
        "fn:IsPresent(A:B)",
    ] {
        assert!(parse(bad).is_err(), "{bad:?} parsed");
    }
}

#[test]
fn a_simple_fonts_widths_are_required_but_of_the_standard_14_before_2_0() {
    let file = bare();
    let cell = &row("FontType1", b"Widths").required;
    let font = |base: &str| {
        Object::Dictionary(
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => base },
        )
    };
    let at = |font: &Object, version| required_in(&file, font, version, cell);
    assert_eq!(at(&font("Helvetica"), "1.7"), Truth::False, "{cell}");
    assert_eq!(at(&font("Helvetica"), "2.0"), Truth::True);
    assert_eq!(at(&font("ArialMT"), "1.7"), Truth::True);
    let untyped = Object::Dictionary(dictionary! { "Subtype" => "Type1", "BaseFont" => "ArialMT" });
    assert_eq!(at(&untyped, "1.7"), Truth::False, "not /Type /Font");
}

#[test]
fn bits_are_the_rows_own_values() {
    let file = bare();
    // A button field is a push button, a checkbox or a radio button by
    // its /Ff's bits 16 and 17 (ISO 32000-1 12.7.4.2).
    let ff = |table: &str| &row(table, b"Ff").special_case[0];
    let field = |ff: i64| Object::Dictionary(dictionary! { "FT" => "Btn", "Ff" => ff });
    let at = |table: &str, flags: i64| {
        file.eval(&field(flags), At::Key(b"Ff"), &[], "1.7", |cx| {
            holds(ff(table), cx)
        })
    };
    for (flags, push, checkbox, radio) in [
        (0, false, true, false),
        (1 << 16, true, false, false),
        ((1 << 15) | (1 << 14), false, false, true),
    ] {
        let expected = [push, checkbox, radio].map(|b| if b { Truth::True } else { Truth::False });
        let got = ["FieldBtnPush", "FieldBtnCheckbox", "FieldBtnRadio"].map(|t| at(t, flags));
        assert_eq!(got, expected, "/Ff {flags}");
    }

    // An encryption dictionary's /P is negative: its bits 13 to 32 set.
    let p = &row("EncryptionStandard", b"P").special_case[0];
    let allowed = |value: i64| {
        let encrypt = Object::Dictionary(dictionary! { "P" => value });
        file.eval(&encrypt, At::Key(b"P"), &[], "1.7", |cx| holds(p, cx))
    };
    assert_eq!(allowed(-3904), Truth::True, "{p}");
    assert_eq!(allowed(-3901), Truth::False, "bits 1 and 2 set");
    assert_eq!(allowed(3904), Truth::False, "bits 13 to 32 clear");
    assert_eq!(allowed(1 << 40), Truth::False, "past 32 bits");

    let x = Object::Dictionary(dictionary! { "F" => 4 });
    let is = |text: &str| file.eval(&x, At::Key(b"F"), &[], "1.7", |cx| holds(text, cx));
    assert_eq!(is("fn:Eval(fn:BitSet(3))"), Truth::True);
    assert_eq!(is("fn:Eval(fn:BitsSet(2,3))"), Truth::False);
    assert_eq!(is("fn:Eval(fn:BitSet(33))"), unknown("fn:BitSet"));
}
