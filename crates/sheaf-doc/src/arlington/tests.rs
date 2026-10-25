use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::grammar::HEADER;
use super::*;

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

#[test]
fn every_table_reads() {
    let g = grammar();
    assert_eq!(g.tables().count(), 613);
    let files = files();
    assert_eq!(files.len(), 613);
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        let mut lines = text.lines();
        assert_eq!(
            lines.next(),
            Some(HEADER.join("\t").as_str()),
            "{}",
            path.display()
        );
        for (i, line) in lines.enumerate() {
            assert_eq!(
                line.split('\t').count(),
                12,
                "{}: line {}",
                path.display(),
                i + 2
            );
        }
    }
    let mut used = BTreeSet::new();
    for table in g.tables() {
        for row in &table.rows {
            let at = format!("{}: line {}", table.name, row.line);
            let n = row.r#type.len();
            assert!(n > 0, "{at}");
            assert_eq!(row.indirect_reference.len(), n, "{at}");
            assert_eq!(row.possible_values.len(), n, "{at}");
            assert_eq!(row.special_case.len(), n, "{at}");
            assert_eq!(row.link.len(), n, "{at}");
            used.extend(row.r#type.iter().map(|t| t.item));
            for link in row.link.iter().flatten() {
                assert!(g.table(&link.item).is_some(), "{at}: {}", link.item);
            }
            // An array, dictionary or stream always links to its tables, a
            // simple type never (INTERNAL_GRAMMAR.md, "Column 11 - Link").
            for (ty, links) in row.r#type.iter().zip(&row.link) {
                match ty.item {
                    ArlType::Array | ArlType::Dictionary | ArlType::Stream => {
                        assert!(!links.is_empty(), "{at}: {} links nowhere", ty.item.name())
                    }
                    ArlType::NameTree | ArlType::NumberTree => {}
                    simple => assert!(links.is_empty(), "{at}: {} links", simple.name()),
                }
            }
        }
    }
    assert_eq!(
        used,
        BTreeSet::from(ArlType::ALL),
        "the model uses every type and no other"
    );
}

#[test]
fn the_vendored_commit_is_the_one_named() {
    let source = Path::new(DIR).join("../../SOURCE.md");
    let source = std::fs::read_to_string(&source).unwrap();
    assert!(source.contains(COMMIT), "SOURCE.md doesn't name {COMMIT}");
}

#[test]
fn rows_are_found_by_key_index_repeat_or_wildcard() {
    let g = grammar();
    let key = |table: &str, at: At| g.table(table).unwrap().row(at).map(|r| r.key.as_str());
    assert_eq!(key("Catalog", At::Key(b"Pages")), Some("Pages"));
    assert_eq!(
        key("Catalog", At::Key(b"Nonesuch")),
        None,
        "Catalog has no wildcard"
    );
    assert_eq!(key("DocInfo", At::Key(b"Nonesuch")), Some("*"));
    // `0`, then the pairs `1*` and `2*` (INTERNAL_GRAMMAR.md, "Column 1 - Key").
    let inks: Vec<_> = (0..6)
        .map(|i| key("ArrayOfOPI2Inks", At::Index(i)))
        .collect();
    let expected = ["0", "1*", "2*", "1*", "2*", "1*"];
    assert_eq!(inks, expected.map(Some));
    assert_eq!(key("ArrayOfQuadPoints", At::Index(17)), Some("1*"));
    assert_eq!(key("ArrayOfAnnots", At::Index(4)), Some("*"));
}

#[test]
fn version_predicates_come_off_types_values_and_links() {
    let g = grammar();
    let row = |table: &str, key: &[u8]| g.table(table).unwrap().row(At::Key(key)).unwrap();
    let wrap = |func, arg: &str| Wrap {
        func,
        arg: arg.to_string(),
    };

    let n = row("Appearance", b"N");
    let types: Vec<_> = n.r#type.iter().map(|t| (t.item, t.wraps.clone())).collect();
    assert_eq!(
        types,
        [
            (ArlType::Dictionary, vec![wrap(WrapFn::SinceVersion, "1.3")]),
            (ArlType::Stream, vec![]),
        ]
    );
    assert_eq!(
        n.link[1]
            .iter()
            .map(|l| l.item.as_str())
            .collect::<Vec<_>>(),
        [
            "XObjectFormType1",
            "XObjectFormPS",
            "XObjectFormPSpassthrough"
        ]
    );

    let filter = row("XObjectImage", b"Filter");
    let flate = filter.possible_values[1]
        .iter()
        .find(|v| v.item == "FlateDecode")
        .unwrap();
    assert_eq!(flate.wraps, [wrap(WrapFn::SinceVersion, "1.2")]);
    assert!(
        filter.possible_values[0].is_empty(),
        "an array of filters has no values listed"
    );

    let annots = g.table("ArrayOfAnnots").unwrap().row(At::Index(0)).unwrap();
    let projection = annots.link[0]
        .iter()
        .find(|l| l.item == "AnnotProjection")
        .unwrap();
    assert_eq!(
        projection.wraps,
        [
            wrap(WrapFn::IsPDFVersion, "1.7"),
            wrap(WrapFn::Extension, "ADBE_Extn3")
        ]
    );
}

/// A copy of the vendored model of the test's own, removed when it drops.
struct Scratch(PathBuf);

impl Scratch {
    fn copy() -> Scratch {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("sheaf-arlington-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for path in files() {
            std::fs::copy(&path, dir.join(path.file_name().unwrap())).unwrap();
        }
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A table that breaks a rule doesn't load, and the error names its
/// file, the line and the rule.
#[test]
fn a_broken_table_says_where() {
    let catalog = std::fs::read_to_string(Path::new(DIR).join("Catalog.tsv")).unwrap();
    let line = catalog.lines().count() + 1;
    for (what, row, why) in [
        (
            "a field short",
            "Bogus\tname\t1.0\t\tFALSE\tFALSE\tFALSE\t\t\t\t",
            "11 fields, not 12",
        ),
        (
            "a type that isn't",
            "Bogus\tdict\t1.0\t\tFALSE\tFALSE\tFALSE\t\t\t\t[]\t",
            "Type \"dict\" isn't one of Arlington's",
        ),
        (
            "a link for one type of two",
            "Bogus\tarray;dictionary\t1.0\t\tFALSE\tFALSE\tFALSE\t\t\t\t[ArrayOfAnnots]\t",
            "Link has 1 items for 2 types",
        ),
        (
            "a link to a missing table",
            "Bogus\tdictionary\t1.0\t\tFALSE\tFALSE\tFALSE\t\t\t\t[Nonesuch]\t",
            "links to Nonesuch, which has no table",
        ),
        (
            "a type's version that isn't one",
            "Bogus\tfn:SinceVersion(1.9,name)\t1.0\t\tFALSE\tFALSE\tFALSE\t\t\t\t[]\t",
            "\"fn:SinceVersion(1.9,name)\": a version predicate that doesn't read",
        ),
        (
            "a value's version that isn't one",
            "Bogus\tname\t1.0\t\tFALSE\tFALSE\tFALSE\t\t[A,fn:SinceVersion(3,B)]\t\t[]\t",
            "\"fn:SinceVersion(3,B)\": a version predicate that doesn't read",
        ),
    ] {
        let scratch = Scratch::copy();
        std::fs::write(scratch.0.join("Catalog.tsv"), format!("{catalog}{row}\n")).unwrap();
        let Err(e) = Grammar::load(&scratch.0) else {
            panic!("{what}: the model still read");
        };
        let e = format!("{e:#}");
        assert!(
            e.contains("Catalog.tsv") && e.contains(&format!("line {line}:")) && e.contains(why),
            "{what}: {e}"
        );
    }
}
