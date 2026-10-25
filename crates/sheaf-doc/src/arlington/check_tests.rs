//! The walk: a clean file reaches its tables with no findings, written
//! with a table and with a cross-reference stream; each rule's finding on
//! a fault planted in one; links chosen as TestGrammar chooses them; only
//! held objects found in; and every walk bounded, on cycles and on deep
//! nesting.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use in_progress::NoProgress;
use lopdf::{Dictionary, Document as Pdf, LazyPdf, Object, Stream, XrefKind, dictionary};

use super::check::{NESTING, is_date};
use super::*;
use crate::materialize::ObjKey;
use crate::materialize::emit::PdfVersion;
use crate::serialize::fixture::{USER, rc4_aes_128_and_aes_256, written as lopdf_written};
use crate::serialize::full::{Body, Layout, write_bodies, write_built};
use crate::source::Limits;

/// `objects`, the catalog object 1, written as sheaf writes a file it
/// built, at `version` and with a section of `kind`.
fn written(version: &str, objects: &[(u32, Object)], kind: XrefKind) -> Vec<u8> {
    written_with(version, objects, kind, Dictionary::new())
}

/// [`written`], with `trailer`'s entries in the trailer too.
fn written_with(
    version: &str,
    objects: &[(u32, Object)],
    kind: XrefKind,
    trailer: Dictionary,
) -> Vec<u8> {
    let mut pdf = Pdf::with_version(version);
    for (n, o) in objects {
        pdf.objects.insert((*n, 0), o.clone());
    }
    pdf.trailer = trailer;
    pdf.trailer.set("Root", r(1));
    match kind {
        XrefKind::Table => write_built(&pdf).unwrap(),
        XrefKind::Stream => {
            let bodies = (pdf.objects.iter())
                .map(|(&id, o)| Ok((id, Body::Object(Cow::Borrowed(o), ObjKey::Built(id)))));
            let layout = Layout {
                trailer: &pdf.trailer,
                kind,
                version: PdfVersion::parse(version.as_bytes()).unwrap(),
                crypt: None,
                encrypt_at: (0, 0),
                xref_from: 0,
            };
            write_bodies(bodies, &layout, &NoProgress).unwrap().0
        }
    }
}

fn open(bytes: &[u8]) -> LazyPdf {
    LazyPdf::open(Arc::<[u8]>::from(bytes), None, &Limits::DEFAULT.reader).unwrap()
}

/// `bytes` checked, holding what `hold` does.
fn checked_as(bytes: &[u8], hold: &Hold) -> Report {
    check(&open(bytes), bytes, hold)
}

/// `objects` written at 1.7 with a table and checked, every object held
/// as built.
fn checked(objects: &[(u32, Object)]) -> Report {
    let bytes = written("1.7", objects, XrefKind::Table);
    checked_as(&bytes, &Hold::All(Made::Built))
}

fn r(n: u32) -> Object {
    Object::Reference((n, 0))
}

fn numbers(n: &[i64]) -> Object {
    Object::Array(n.iter().map(|&n| Object::Integer(n)).collect())
}

fn text(s: &str) -> Object {
    Object::string_literal(s)
}

/// The tables `checked` has, and how many of each.
fn tables(checked: &[(&str, usize)]) -> BTreeMap<String, usize> {
    (checked.iter())
        .map(|&(table, n)| (table.to_string(), n))
        .collect()
}

/// A clean file: a catalog, its page tree, a page with a Text
/// annotation, and the annotation's appearance stream with its resources.
fn clean() -> Vec<(u32, Object)> {
    let form = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => numbers(&[0, 0, 20, 20]),
        "Resources" => dictionary! { "ProcSet" => vec![Object::Name(b"PDF".to_vec())] },
    };
    vec![
        (
            1,
            Object::Dictionary(dictionary! { "Type" => "Catalog", "Pages" => r(2) }),
        ),
        (
            2,
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => vec![r(3)], "Count" => 1 },
            ),
        ),
        (
            3,
            Object::Dictionary(dictionary! {
                "Type" => "Page",
                "Parent" => r(2),
                "MediaBox" => numbers(&[0, 0, 612, 792]),
                "Resources" => dictionary! {},
                "Annots" => vec![r(4)],
            }),
        ),
        (
            4,
            Object::Dictionary(dictionary! {
                "Type" => "Annot",
                "Subtype" => "Text",
                "Rect" => numbers(&[100, 100, 120, 120]),
                "Contents" => text("A note"),
                "AP" => dictionary! { "N" => r(5) },
            }),
        ),
        (
            5,
            Object::Stream(Stream::new(form, b"0 0 m 20 20 l S".to_vec())),
        ),
    ]
}

/// `objects` with object `n`'s dictionary as `f` changes it.
fn changed(
    mut objects: Vec<(u32, Object)>,
    n: u32,
    f: impl FnOnce(&mut Dictionary),
) -> Vec<(u32, Object)> {
    match objects.iter_mut().find(|(m, _)| *m == n).map(|(_, o)| o) {
        Some(Object::Dictionary(d)) => f(d),
        Some(Object::Stream(s)) => f(&mut s.dict),
        _ => panic!("no dictionary {n}"),
    }
    objects
}

/// A finding in object `n` (built), as the tests expect one.
fn finding(n: u32, path: &str, (table, key): (&str, &str), rule: Rule, value: &str) -> Finding {
    Finding {
        object: Some((n, 0)),
        made: Made::Built,
        path: path.to_string(),
        table: table.to_string(),
        key: key.to_string(),
        rule,
        value: value.to_string(),
    }
}

fn unlisted(table: &str, key: &str, class: Class) -> Unlisted {
    Unlisted {
        table: table.to_string(),
        key: key.to_string(),
        class,
    }
}

/// Each finding's object, table, key, rule and value.
fn found(report: &Report) -> Vec<(Option<u32>, &str, &str, Rule, &str)> {
    (report.findings.iter())
        .map(|f| {
            let object = f.object.map(|(n, _)| n);
            (
                object,
                f.table.as_str(),
                f.key.as_str(),
                f.rule,
                f.value.as_str(),
            )
        })
        .collect()
}

#[test]
fn a_clean_file_reaches_its_tables_with_no_findings() {
    let common = [
        ("Catalog", 1),
        ("PageTreeNodeRoot", 1),
        ("ArrayOfPageTreeNodeKids", 1),
        ("PageObject", 1),
        ("Resource", 2),
        ("ArrayOfAnnots", 1),
        ("AnnotText", 1),
        ("Appearance", 1),
        ("XObjectFormType1", 1),
        ("ArrayOfNamesForProcSet", 1),
    ];
    // Each key and element: the trailer's two, the catalog's 2, the
    // page tree's 3 and its kid, the page's 5 and its annotation, the
    // annotation's 5 and its appearance, the form's 5, its resources' 1
    // and its ProcSet's 1. A cross-reference stream has 4 more keys than
    // a table's trailer, and its /W's 3 elements and /Index's 2.
    for (kind, trailer, keys) in [
        (XrefKind::Table, &[("FileTrailer", 1)][..], 27),
        (
            XrefKind::Stream,
            &[
                ("XRefStream", 1),
                ("ArrayOfXRefWIntegers", 1),
                ("ArrayOfXRefIndexIntegers", 1),
            ][..],
            36,
        ),
    ] {
        let bytes = written("1.7", &clean(), kind);
        let report = checked_as(&bytes, &Hold::All(Made::Built));
        assert_eq!(report.findings, [], "{kind:?}:\n{report}");
        assert_eq!(report.unlisted, BTreeMap::new(), "{kind:?}:\n{report}");
        let mut expected = tables(&common);
        expected.extend(tables(trailer));
        assert_eq!(report.checked, expected, "{kind:?}:\n{report}");
        assert_eq!(report.keys, keys, "{kind:?}:\n{report}");
        let counts = (
            report.ties,
            report.no_table,
            report.truncated,
            report.two_contexts,
        );
        assert_eq!(counts, (0, 0, 0, 0), "{report}");
    }
}

#[test]
fn a_value_of_a_type_its_row_doesnt_allow_is_found() {
    let bogus = changed(clean(), 4, |annot| annot.set("Rect", "Bogus"));
    assert_eq!(
        checked(&bogus).findings,
        [finding(
            4,
            "Root/Pages/Kids[0]/Annots[0]",
            ("AnnotText", "Rect"),
            Rule::Type,
            "/Bogus"
        )]
    );

    // A rectangle is four numbers, and a date a date.
    let refined = changed(clean(), 4, |annot| annot.set("Rect", numbers(&[1, 2, 3])));
    let refined = changed(refined, 3, |page| {
        page.set("LastModified", text("D:20261324"))
    });
    let report = checked(&refined);
    assert_eq!(
        found(&report),
        [
            (
                Some(3),
                "PageObject",
                "LastModified",
                Rule::Type,
                "(D:20261324)"
            ),
            (Some(4), "AnnotText", "Rect", Rule::Type, "[1 2 3]"),
        ],
        "{report}"
    );

    // A type its row allows only from a version later than the file's.
    let states = changed(clean(), 4, |annot| {
        annot.set("AP", dictionary! { "N" => dictionary! { "Off" => r(5) } });
        annot.set("AS", "Off");
    });
    let bytes = written("1.2", &states, XrefKind::Table);
    let report = checked_as(&bytes, &Hold::All(Made::Built));
    let types: Vec<_> = (found(&report).into_iter())
        .filter(|f| f.3 == Rule::Type)
        .collect();
    assert_eq!(
        types,
        [(
            Some(4),
            "Appearance",
            "N",
            Rule::Type,
            "<< /Off 5 0 R >> (dictionary isn't allowed at 1.2)"
        )],
        "{report}"
    );
}

#[test]
fn a_cross_reference_streams_keys_are_checked_as_the_trailer() {
    // lopdf reads `/W` only as three integers, refusing the section
    // otherwise, so a bad one leaves no section to check; it reads
    // `/Prev` as an offset only when it's an integer.
    let mut bytes = written("1.7", &clean(), XrefKind::Stream);
    let start = open(&bytes).xref().start;
    let dict = start + bytes[start..].windows(2).position(|w| w == b"<<").unwrap() + 2;
    bytes.splice(dict..dict, b" /Prev /Bogus".iter().copied());
    let pdf = open(&bytes);
    let stream = pdf.xref().size - 1;
    let report = check(&pdf, &bytes, &Hold::All(Made::Built));
    let mut expected = finding(
        stream,
        "trailer",
        ("XRefStream", "Prev"),
        Rule::Type,
        "/Bogus",
    );
    expected.made = Made::Merged;
    assert_eq!(report.findings, [expected], "{report}");
}

#[test]
fn a_trailer_that_doesnt_read_is_found_and_the_file_walked_as_read() {
    // `startxref` pointing past the file: the reader rebuilds the table
    // by scanning, and makes a trailer of the one it finds.
    let open_yes = changed(clean(), 4, |annot| annot.set("Open", text("yes")));
    let mut bytes = written("1.7", &open_yes, XrefKind::Table);
    let at = bytes.windows(9).rposition(|w| w == b"startxref").unwrap() + 9;
    let end = at + bytes[at..].iter().position(|b| b.is_ascii_digit()).unwrap();
    let digits = bytes[end..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    bytes.splice(end..end + digits, b"99999999".iter().copied());
    let pdf = open(&bytes);
    assert!(!pdf.base_sound());
    let report = check(&pdf, &bytes, &Hold::All(Made::Built));
    let rules: Vec<_> = (report.findings.iter())
        .map(|f| (f.object, f.table.as_str(), f.key.as_str(), f.rule))
        .collect();
    assert_eq!(
        rules,
        [
            (None, "FileTrailer", "-", Rule::Unresolved),
            (Some((4, 0)), "AnnotText", "Open", Rule::Type),
        ],
        "{report}"
    );
    for table in ["FileTrailer", "Catalog", "PageObject", "AnnotText"] {
        assert_eq!(report.checked.get(table), Some(&1), "{table}: {report}");
    }

    // A cross-reference stream its own section doesn't list, past its
    // `/Size` and `/Index`, as some writers leave it: read from the bytes,
    // as written.
    let mut bytes = written("1.7", &open_yes, XrefKind::Stream);
    let pdf = open(&bytes);
    let (size, start) = (pdf.xref().size, pdf.xref().start);
    let stream = (size - 1, 0);
    let dict = start..start + bytes[start..].windows(2).position(|w| w == b">>").unwrap();
    let text = String::from_utf8(bytes[dict.clone()].to_vec()).unwrap();
    let fewer = (text.replace(&format!("/Size {size}"), &format!("/Size {}", size - 1)))
        .replace(&format!("[0 {size}]"), &format!("[0 {}]", size - 1));
    assert_ne!(fewer, text);
    bytes.splice(
        dict,
        format!("{fewer:<width$}", width = text.len()).into_bytes(),
    );
    let pdf = open(&bytes);
    assert!(pdf.base_sound() && pdf.get(stream).is_err());
    let report = check(&pdf, &bytes, &Hold::All(Made::Built));
    let rules: Vec<_> = (report.findings.iter())
        .map(|f| (f.object, f.table.as_str(), f.key.as_str(), f.rule))
        .collect();
    assert_eq!(
        rules,
        [
            (None, "FileTrailer", "-", Rule::Unresolved),
            (Some((4, 0)), "AnnotText", "Open", Rule::Type),
        ],
        "{report}"
    );
    assert_eq!(report.trailer, Some(stream), "{report}");
    assert_eq!(report.checked.get("XRefStream"), Some(&1), "{report}");
}

#[test]
fn a_required_key_absent_is_found_unless_inherited() {
    let no_rect = changed(clean(), 4, |annot| {
        annot.remove(b"Rect");
    });
    assert_eq!(
        checked(&no_rect).findings,
        [finding(
            4,
            "Root/Pages/Kids[0]/Annots[0]",
            ("AnnotText", "Rect"),
            Rule::Missing,
            "absent"
        )]
    );

    // A page's MediaBox is inheritable: its parent's will do.
    let no_box = changed(clean(), 3, |page| {
        page.remove(b"MediaBox");
    });
    assert_eq!(
        checked(&no_box).findings,
        [finding(
            3,
            "Root/Pages/Kids[0]",
            ("PageObject", "MediaBox"),
            Rule::Missing,
            "absent"
        )]
    );
    let inherited = changed(no_box, 2, |pages| {
        pages.set("MediaBox", numbers(&[0, 0, 612, 792]))
    });
    let report = checked(&inherited);
    assert_eq!(report.findings, [], "{report}");

    // A text field's /DA is inheritable: its parent's will do, and so will
    // the form's, the document-wide default.
    let field = |form: Dictionary, parent: Dictionary| {
        let mut file = changed(clean(), 1, |catalog| {
            let mut form = form;
            form.set("Fields", vec![r(7)]);
            catalog.set("AcroForm", form);
        });
        let mut parent = parent;
        parent.extend(&dictionary! { "FT" => "Tx", "T" => text("p"), "Kids" => vec![r(6)] });
        file.extend([
            (
                6,
                Object::Dictionary(
                    dictionary! { "FT" => "Tx", "T" => text("t"), "Parent" => r(7) },
                ),
            ),
            (7, Object::Dictionary(parent)),
        ]);
        checked(&file)
    };
    let report = field(dictionary! {}, dictionary! {});
    assert_eq!(
        found(&report),
        [
            (Some(7), "FieldTx", "DA", Rule::Missing, "absent"),
            (Some(6), "FieldTx", "DA", Rule::Missing, "absent"),
        ],
        "{report}"
    );
    let da = || dictionary! { "DA" => text("/Helv 0 Tf 0 g") };
    for report in [field(da(), dictionary! {}), field(dictionary! {}, da())] {
        assert_eq!(report.findings, [], "{report}");
    }

    // An array shorter than its table's required rows.
    let line = changed(clean(), 4, |annot| {
        annot.set("Subtype", "Line");
        annot.set("L", numbers(&[1, 2, 3]));
    });
    let report = checked(&line);
    assert_eq!(
        found(&report),
        [(Some(4), "ArrayOf_4Numbers", "3", Rule::Missing, "absent")],
        "{report}"
    );
}

#[test]
fn an_object_is_checked_as_the_table_it_fits_best() {
    let square = changed(clean(), 4, |annot| annot.set("Subtype", "Square"));
    let report = checked(&square);
    assert_eq!(report.findings, [], "{report}");
    assert_eq!(report.checked.get("AnnotSquare"), Some(&1), "{report}");
    assert_eq!(report.checked.get("AnnotText"), None, "{report}");
    assert_eq!(report.ties, 0, "{report}");

    // Without its Subtype, a square's keys fit a circle's table as well:
    // the two have the same rows. The first listed is taken, and counted.
    let ambiguous = changed(clean(), 4, |annot| {
        annot.remove(b"Subtype");
        annot.set("IC", numbers(&[1, 0, 0]));
        annot.set("RD", numbers(&[1, 1, 1, 1]));
    });
    let report = checked(&ambiguous);
    assert_eq!(report.ties, 1, "{report}");
    assert_eq!(report.checked.get("AnnotSquare"), Some(&1), "{report}");

    // A button field is a push button, a checkbox or a radio button by
    // its /Ff's bits alone.
    for (flags, table) in [
        (1 << 16, "FieldBtnPush"),
        (0, "FieldBtnCheckbox"),
        (1 << 15, "FieldBtnRadio"),
    ] {
        let field = dictionary! { "FT" => "Btn", "T" => text("b"), "Ff" => flags };
        let mut form = changed(clean(), 1, |catalog| {
            catalog.set("AcroForm", dictionary! { "Fields" => vec![r(6)] })
        });
        form.push((6, Object::Dictionary(field)));
        let report = checked(&form);
        assert_eq!(report.checked.get(table), Some(&1), "{report}");
        assert_eq!(report.ties, 0, "{report}");
    }

    // A structure parent tree's array is an array's table, even as long
    // as a structure element's table has rows (19).
    let mut tree = changed(clean(), 1, |catalog| catalog.set("StructTreeRoot", r(6)));
    tree.extend([
        (
            6,
            Object::Dictionary(dictionary! {
                "Type" => "StructTreeRoot",
                "K" => r(7),
                "ParentTree" => dictionary! { "Nums" => vec![0.into(), r(8)] },
            }),
        ),
        (
            7,
            Object::Dictionary(dictionary! {
                "Type" => "StructElem",
                "S" => "P",
                "P" => r(6),
                "Pg" => r(3),
                "K" => 0,
            }),
        ),
        (8, Object::Array(vec![r(7); 19])),
    ]);
    let report = checked(&tree);
    assert_eq!(report.findings, [], "{report}");
    let tables = ["ArrayOfStructElemParentTree", "StructElem"];
    for table in tables {
        assert_eq!(report.checked.get(table), Some(&1), "{table}: {report}");
    }
    assert_eq!(report.ties, 0, "{report}");

    // One dictionary reached as a page's resources and as a square's
    // border style keeps the first, counted.
    let shared = changed(square, 3, |page| page.set("Resources", r(6)));
    let mut shared = changed(shared, 4, |annot| annot.set("BS", r(6)));
    shared.push((6, Object::Dictionary(dictionary! {})));
    let report = checked(&shared);
    assert_eq!(report.findings, [], "{report}");
    assert_eq!(report.two_contexts, 1, "{report}");
    assert_eq!(report.checked.get("BorderStyle"), None, "{report}");
}

#[test]
fn an_object_a_universal_table_reaches_first_is_checked_as_another_that_reaches_it() {
    // The catalog's structure tree comes before its pages, and its element
    // marks the annotation (`/OBJR`), whose `/Obj` the model links to
    // `_UniversalDictionary` alone: the walk reaches the annotation there
    // first.
    let mut tree = clean();
    tree[0].1 = Object::Dictionary(dictionary! {
        "Type" => "Catalog", "StructTreeRoot" => r(6), "Pages" => r(2),
    });
    tree.extend([
        (
            6,
            Object::Dictionary(dictionary! { "Type" => "StructTreeRoot", "K" => r(7) }),
        ),
        (
            7,
            Object::Dictionary(dictionary! {
                "Type" => "StructElem", "S" => "Annot", "P" => r(6), "Pg" => r(3),
                "K" => dictionary! { "Type" => "OBJR", "Obj" => r(4), "Pg" => r(3) },
            }),
        ),
    ]);
    let tree = changed(tree, 4, |annot| annot.set("Open", text("yes")));
    let report = checked(&tree);
    assert_eq!(report.checked.get("ObjectReference"), Some(&1), "{report}");
    assert_eq!(report.checked.get("AnnotText"), Some(&1), "{report}");
    assert_eq!(report.checked.get("_UniversalDictionary"), None, "{report}");
    assert_eq!(
        found(&report),
        [(Some(4), "AnnotText", "Open", Rule::Type, "(yes)")],
        "checked as the annotation it is: {report}"
    );

    // Reached as nothing else, it's checked as that table, last.
    let alone = changed(tree, 3, |page| {
        page.remove(b"Annots");
    });
    let report = checked(&alone);
    assert_eq!(report.findings, [], "{report}");
    assert_eq!(report.checked.get("AnnotText"), None, "{report}");
    assert_eq!(
        report.checked.get("_UniversalDictionary"),
        Some(&2),
        "{report}"
    );
}

#[test]
fn only_held_objects_are_found_in_but_all_are_walked() {
    let no_box = changed(clean(), 3, |page| {
        page.remove(b"MediaBox");
    });
    let bytes = written("1.7", &no_box, XrefKind::Table);
    let all = checked_as(&bytes, &Hold::All(Made::Built));
    assert_eq!(all.findings.len(), 1, "{all}");

    let annotation = BTreeMap::from([((4, 0), Made::Merged), ((5, 0), Made::Built)]);
    let report = checked_as(&bytes, &Hold::Only(annotation));
    assert_eq!(report.findings, [], "{report}");
    assert_eq!(
        report.checked,
        tables(&[
            ("FileTrailer", 1),
            ("AnnotText", 1),
            ("Appearance", 1),
            ("XObjectFormType1", 1),
            ("Resource", 1),
            ("ArrayOfNamesForProcSet", 1),
        ]),
        "the page isn't held, and its annotation is still an annotation"
    );

    let bogus = changed(no_box, 4, |annot| annot.set("Rect", "Bogus"));
    let bytes = written("1.7", &bogus, XrefKind::Table);
    let report = checked_as(
        &bytes,
        &Hold::Only(BTreeMap::from([((4, 0), Made::Merged)])),
    );
    let held: Vec<_> = (report.findings.iter())
        .map(|f| (f.object, f.made, f.rule))
        .collect();
    assert_eq!(held, [(Some((4, 0)), Made::Merged, Rule::Type)], "{report}");
}

#[test]
fn a_held_object_nothing_reaches_is_found() {
    let mut objects = changed(clean(), 1, |catalog| catalog.set("SheafGenerators", r(7)));
    objects.extend([
        (
            6,
            Object::Dictionary(
                dictionary! { "Type" => "Annot", "Subtype" => "Text", "Popup" => r(8) },
            ),
        ),
        (7, Object::Dictionary(dictionary! { "Mine" => 1 })),
        (8, Object::Dictionary(dictionary! { "Subtype" => "Popup" })),
    ]);
    let report = checked(&objects);
    let unreached = |n: u32, key: &str| (n, "-", "-", Rule::Unreached, key.to_string());
    let seen: Vec<_> = (report.findings.iter())
        .map(|f| {
            let n = f.object.unwrap().0;
            (n, f.path.as_str(), f.table.as_str(), f.rule, f.key.clone())
        })
        .collect();
    assert_eq!(
        seen,
        [unreached(6, "-"), unreached(8, "Popup")],
        "an object under a key the model doesn't list isn't one: {report}"
    );
    assert_eq!(
        report.unlisted,
        BTreeMap::from([(unlisted("Catalog", "SheafGenerators", Class::Other), 1)])
    );

    // A reference to no object is found where it's written.
    let dangling = changed(clean(), 3, |page| page.set("Annots", vec![r(4), r(9)]));
    assert_eq!(
        checked(&dangling).findings,
        [finding(
            3,
            "Root/Pages/Kids[0]/Annots",
            ("ArrayOfAnnots", "1"),
            Rule::Unresolved,
            "9 0 R"
        )]
    );
}

#[test]
fn name_and_number_trees_are_walked_and_a_malformed_node_is_found() {
    let dest = || Object::Array(vec![r(3), Object::Name(b"Fit".to_vec())]);
    let leaf = dictionary! {
        "Names" => vec![text("a"), dest(), text("b"), dest()],
        "Limits" => vec![text("a"), text("b")],
    };
    let labels =
        dictionary! { "Nums" => vec![0.into(), Object::Dictionary(dictionary! { "S" => "D" })] };
    let mut objects = changed(clean(), 1, |catalog| {
        catalog.set(
            "Names",
            dictionary! { "Dests" => dictionary! { "Kids" => vec![r(6)] } },
        );
        catalog.set("PageLabels", labels);
    });
    objects.push((6, Object::Dictionary(leaf)));
    let report = checked(&objects);
    assert_eq!(report.findings, [], "{report}");
    for (table, n) in [
        ("name-tree", 2),
        ("number-tree", 1),
        ("Dest0Array", 2),
        ("PageLabel", 1),
    ] {
        assert_eq!(report.checked.get(table), Some(&n), "{table}: {report}");
    }

    let broken = changed(objects, 6, |leaf| {
        leaf.remove(b"Limits");
        leaf.set("Names", vec![text("a"), dest(), 3.into()]);
    });
    let report = checked(&broken);
    assert_eq!(
        found(&report),
        [
            (
                Some(6),
                "Name",
                "Dests",
                Rule::Tree,
                "a node under the root without /Limits"
            ),
            (
                Some(6),
                "Name",
                "Dests",
                Rule::Tree,
                "its last key, [2], has no value"
            ),
        ],
        "{report}"
    );
}

#[test]
fn every_walk_ends_on_cycles_and_deep_nesting() {
    // A /Parent cycle, up which the inheritable MediaBox is looked for.
    let cycle = changed(clean(), 3, |page| {
        page.remove(b"MediaBox");
    });
    let cycle = changed(cycle, 2, |pages| pages.set("Parent", r(3)));
    let report = checked(&cycle);
    let missing: Vec<_> = (found(&report).into_iter())
        .filter(|f| f.3 == Rule::Missing)
        .map(|f| (f.1, f.2))
        .collect();
    assert_eq!(missing, [("PageObject", "MediaBox")], "{report}");

    // An array that holds itself, and one nested past the cap, as a page
    // piece's private data.
    let mut deep = Object::Array(Vec::new());
    for _ in 0..NESTING + 6 {
        deep = Object::Array(vec![deep]);
    }
    let piece = dictionary! { "Sheaf" => dictionary! {
        "LastModified" => text("D:20261024"),
        "Private" => r(6),
    } };
    let mut objects = changed(clean(), 3, |page| {
        page.set("LastModified", text("D:20261024"));
        page.set("PieceInfo", piece);
    });
    objects.push((6, Object::Array(vec![r(6), deep])));
    let report = checked(&objects);
    assert_eq!(report.findings, [], "{report}");
    assert_eq!(report.truncated, 1, "{report}");
    assert_eq!(
        report.checked.get("_UniversalArray"),
        Some(&(NESTING + 1)),
        "{report}"
    );
}

/// An outline of 50,000 items, each reached from the one before by its
/// `/Next`: the walk's way to the last is 50,000 steps long, and it's
/// dropped without recursion, on a thread with an eighth of a test
/// thread's stack, which a recursive drop of that many steps overflows.
#[test]
fn a_way_as_long_as_the_file_makes_it_drops_on_a_small_stack() {
    const ITEMS: u32 = 50_000;
    const STACK: usize = 256 << 10;
    let first = 7;
    let last = first + ITEMS - 1;
    let mut objects = changed(clean(), 1, |catalog| catalog.set("Outlines", r(6)));
    objects.push((
        6,
        Object::Dictionary(dictionary! {
            "Type" => "Outlines", "First" => r(first), "Last" => r(last), "Count" => ITEMS,
        }),
    ));
    for n in first..=last {
        let mut item = dictionary! { "Title" => text("An item"), "Parent" => r(6) };
        if n > first {
            item.set("Prev", r(n - 1));
        }
        if n < last {
            item.set("Next", r(n + 1));
        }
        objects.push((n, Object::Dictionary(item)));
    }
    let bytes = written("1.7", &objects, XrefKind::Table);
    let report = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || checked_as(&bytes, &Hold::All(Made::Built)))
        .unwrap()
        .join()
        .expect("the walk ends");
    assert_eq!(report.findings, [], "{report}");
    assert_eq!(
        report.checked.get("OutlineItem"),
        Some(&(ITEMS as usize)),
        "{report}"
    );
}

#[test]
fn a_date_reads_as_test_grammar_reads_one() {
    for (date, valid) in [
        ("D:2026", true),
        ("D:20261024", true),
        ("D:20261024153000Z", true),
        ("D:20261024153000+05'30'", true),
        ("D:20261024153000-08'00", true),
        ("D:2026102415", true),
        ("D:202610241530007", true),
        ("D:20261324", false),
        ("D:20261000", false),
        ("D:20261024246000", false),
        ("D:20261024153000+24'00'", false),
        ("D:20261024153000+05'60'", false),
        ("D:2026102415300099", false),
        ("D:26", false),
        ("20261024", false),
    ] {
        assert_eq!(is_date(date), valid, "{date}");
    }
}

#[test]
fn a_structure_trees_arrays_are_read_as_test_grammar_reads_them() {
    let structure = |attributes: Vec<Object>| {
        let mut objects = changed(clean(), 1, |catalog| catalog.set("StructTreeRoot", r(6)));
        let reference = dictionary! { "Type" => "OBJR", "Obj" => r(4) };
        objects.extend([
            (
                6,
                Object::Dictionary(dictionary! { "Type" => "StructTreeRoot", "K" => r(7) }),
            ),
            (
                7,
                Object::Dictionary(dictionary! {
                    "Type" => "StructElem",
                    "S" => "Form",
                    "P" => r(6),
                    "Pg" => r(3),
                    "A" => attributes,
                    "K" => vec![Object::Dictionary(reference)],
                }),
            ),
        ]);
        objects
    };
    // An attribute object's revision number may be left out: an element
    // that isn't one is the next attribute object.
    let table = || Object::Dictionary(dictionary! { "O" => "Table" });
    for attributes in [vec![table(), table()], vec![table(), 3.into(), table()]] {
        let report = checked(&structure(attributes));
        assert_eq!(report.findings, [], "{report}");
    }

    // An object reference in a /K array: its parent is the array, which
    // has no /Pg to read, so whether it needs its own can't be told.
    let report = checked(&structure(vec![table()]));
    let unchecked = Unchecked {
        column: "Required",
        why: "parent::Pg".to_string(),
        table: "ObjectReference".to_string(),
        key: "Pg".to_string(),
    };
    assert_eq!(report.unchecked.get(&unchecked), Some(&1), "{report}");
}

/// `clean()`'s appearance stream written with `/Length 6 0 R`, object 6
/// `length`, and `more` beside, at 1.7 with a table.
fn length_referenced(length: Object, more: &[(u32, Object)]) -> Vec<u8> {
    let data = b"0 0 m 20 20 l S";
    let mut raw = b"5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] \
        /Resources << >> /Length 6 0 R >>\nstream\n"
        .to_vec();
    raw.extend_from_slice(data);
    raw.extend_from_slice(b"\nendstream\nendobj");
    let objects: Vec<_> = (clean().into_iter())
        .filter(|(n, _)| *n != 5)
        .chain([(6, length)])
        .chain(more.iter().cloned())
        .collect();
    let mut bodies: Vec<_> = (objects.iter())
        .map(|(n, o)| {
            (
                (*n, 0),
                Body::Object(Cow::Borrowed(o), ObjKey::Built((*n, 0))),
            )
        })
        .collect();
    bodies.push(((5, 0), Body::Raw(&raw)));
    bodies.sort_by_key(|(id, _)| *id);
    let trailer = dictionary! { "Root" => r(1) };
    let layout = Layout {
        trailer: &trailer,
        kind: XrefKind::Table,
        version: PdfVersion::new(1, 7),
        crypt: None,
        encrypt_at: (0, 0),
        xref_from: 0,
    };
    write_bodies(bodies.into_iter().map(Ok), &layout, &NoProgress)
        .unwrap()
        .0
}

#[test]
fn a_streams_referenced_length_is_reached() {
    // lopdf reads a referenced /Length into the stream's dictionary: the
    // reference is read from the stream's bytes, as written.
    let bytes = length_referenced(Object::Integer(15), &[]);
    let report = checked_as(&bytes, &Hold::All(Made::Built));
    assert_eq!(report.findings, [], "{report}");
    assert_eq!(report.checked.get("XObjectFormType1"), Some(&1), "{report}");

    // A length that isn't an integer isn't one, whether or not another
    // integer is left unreached.
    let unreached = |report: &Report| {
        (report.findings.iter())
            .filter(|f| f.rule == Rule::Unreached)
            .map(|f| f.object.unwrap().0)
            .collect::<Vec<_>>()
    };
    let real = length_referenced(Object::Real(15.0), &[]);
    let report = checked_as(&real, &Hold::All(Made::Built));
    assert_eq!(unreached(&report), [6], "{report}");
    let and_an_integer = length_referenced(Object::Real(15.0), &[(7, Object::Integer(3))]);
    let report = checked_as(&and_an_integer, &Hold::All(Made::Built));
    assert_eq!(unreached(&report), [6, 7], "{report}");
}

#[test]
fn a_key_later_than_the_files_version_is_found() {
    let opacity = changed(clean(), 4, |annot| annot.set("CA", 0.5));
    let old = written("1.3", &opacity, XrefKind::Table);
    let report = checked_as(&old, &Hold::All(Made::Built));
    assert_eq!(
        report.findings,
        [finding(
            4,
            "Root/Pages/Kids[0]/Annots[0]",
            ("AnnotText", "CA"),
            Rule::TooNew,
            "0.5 (since 1.4)"
        )]
    );

    let report = checked_as(
        &written("1.4", &opacity, XrefKind::Table),
        &Hold::All(Made::Built),
    );
    assert_eq!(report.findings, [], "{report}");
    // The catalog's /Version, when later, is the file's.
    let later = changed(opacity, 1, |catalog| catalog.set("Version", "1.4"));
    let report = checked_as(
        &written("1.3", &later, XrefKind::Table),
        &Hold::All(Made::Built),
    );
    assert_eq!(report.findings, [], "{report}");
}

#[test]
fn a_value_its_row_doesnt_list_is_found() {
    let bogus = changed(clean(), 4, |annot| {
        annot.set("Name", "Bogus");
        annot.set("CA", 1.5);
    });
    let report = checked(&bogus);
    assert_eq!(
        found(&report),
        [
            (
                Some(4),
                "AnnotText",
                "Name",
                Rule::Value,
                "/Bogus (not one of Comment, Key, Note, Help, NewParagraph, Paragraph, Insert)"
            ),
            (
                Some(4),
                "AnnotText",
                "CA",
                Rule::Value,
                "1.5 (not one of fn:Eval((@CA>=0.0) && (@CA<=1.0)))"
            ),
        ],
        "{report}"
    );

    // A stamp's name may be any, its row's `*` says.
    let stamp = changed(clean(), 4, |annot| {
        annot.set("Subtype", "Stamp");
        annot.set("Name", "SheafReviewed");
    });
    let report = checked(&stamp);
    assert_eq!(report.findings, [], "{report}");

    // A page's tab order /A is an extension's at 1.7 and PDF's from 2.0.
    let tabs = changed(clean(), 3, |page| page.set("Tabs", "A"));
    let report = checked(&tabs);
    assert_eq!(
        found(&report),
        [(
            Some(3),
            "PageObject",
            "Tabs",
            Rule::Value,
            "/A (not one of R, C, S)"
        )],
        "{report}"
    );
    let id = dictionary! { "ID" => vec![text("0123"), text("0123")] };
    let bytes = written_with("2.0", &tabs, XrefKind::Table, id);
    let report = checked_as(&bytes, &Hold::All(Made::Built));
    let values: Vec<_> = (found(&report).into_iter())
        .filter(|f| f.3 == Rule::Value)
        .collect();
    assert_eq!(values, [], "{report}");

    // An image's mask's /Decode is one of two arrays.
    for (decode, values) in [
        (numbers(&[1, 0]), vec![]),
        (
            numbers(&[0, 2]),
            vec![(
                Some(7),
                "XObjectImageMask",
                "Decode",
                Rule::Value,
                "[0 2] (not one of [0 1], [1 0])",
            )],
        ),
    ] {
        let image = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 1,
            "Height" => 1,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
            "Mask" => r(7),
        };
        let mask = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 1,
            "Height" => 1,
            "ImageMask" => true,
            "Decode" => decode,
        };
        let mut objects = changed(clean(), 3, |page| {
            page.set(
                "Resources",
                dictionary! { "XObject" => dictionary! { "Im0" => r(6) } },
            )
        });
        objects.push((6, Object::Stream(Stream::new(image, vec![0]))));
        objects.push((7, Object::Stream(Stream::new(mask, vec![0]))));
        let report = checked(&objects);
        assert_eq!(found(&report), values, "{report}");
        assert_eq!(report.checked.get("XObjectImageMask"), Some(&1), "{report}");
    }

    // An image's bits per component may have to be one value, by
    // fn:RequiredValue, which isn't supported: none of them is checked.
    let image = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Image",
        "Width" => 1,
        "Height" => 1,
        "ColorSpace" => "DeviceGray",
        "BitsPerComponent" => 3,
    };
    let mut objects = changed(clean(), 3, |page| {
        page.set(
            "Resources",
            dictionary! { "XObject" => dictionary! { "Im0" => r(6) } },
        )
    });
    objects.push((6, Object::Stream(Stream::new(image, vec![0]))));
    let report = checked(&objects);
    assert_eq!(report.findings, [], "{report}");
    let unchecked = Unchecked {
        column: "PossibleValues",
        why: "fn:RequiredValue".to_string(),
        table: "XObjectImage".to_string(),
        key: "BitsPerComponent".to_string(),
    };
    assert_eq!(report.unchecked.get(&unchecked), Some(&1), "{report}");

    // The document information dictionary's /Trapped.
    let info = Object::Dictionary(dictionary! { "Trapped" => "Maybe" });
    let objects: Vec<_> = clean().into_iter().chain([(6, info)]).collect();
    let trailer = dictionary! { "Info" => r(6) };
    let bytes = written_with("1.7", &objects, XrefKind::Table, trailer);
    let report = checked_as(&bytes, &Hold::All(Made::Built));
    assert_eq!(
        found(&report),
        [(
            Some(6),
            "DocInfo",
            "Trapped",
            Rule::Value,
            "/Maybe (not one of True, False, Unknown)"
        )],
        "{report}"
    );
}

#[test]
fn a_deprecated_key_type_or_value_is_found() {
    let blend = |bm: Object| {
        changed(clean(), 5, |form| {
            form.set(
                "Resources",
                dictionary! { "ExtGState" => dictionary! { "G0" => dictionary! { "BM" => bm } } },
            )
        })
    };
    // PDF 2.0 asks every file for an /ID.
    let id = || dictionary! { "ID" => vec![text("0123"), text("0123")] };
    let info = Object::Dictionary(dictionary! { "Title" => text("Mine") });
    let objects: Vec<_> = (blend("Compatible".into()).into_iter())
        .chain([(6, info)])
        .collect();
    let mut trailer = id();
    trailer.set("Info", r(6));
    let bytes = written_with("2.0", &objects, XrefKind::Table, trailer);
    let report = checked_as(&bytes, &Hold::All(Made::Built));
    assert_eq!(
        found(&report),
        [
            (
                Some(6),
                "DocInfo",
                "Title",
                Rule::Deprecated,
                "(Mine) (deprecated in 2.0)"
            ),
            (
                Some(5),
                "GraphicsStateParameter",
                "BM",
                Rule::Deprecated,
                "/Compatible (deprecated in 1.4)"
            ),
        ],
        "{report}"
    );

    // A blend mode as an array is deprecated from 2.0, as a type, and so
    // is each element of one, as a row.
    let array = blend(vec![Object::Name(b"Normal".to_vec())].into());
    let bytes = written_with("2.0", &array, XrefKind::Table, id());
    let report = checked_as(&bytes, &Hold::All(Made::Built));
    assert_eq!(
        found(&report),
        [
            (
                Some(5),
                "GraphicsStateParameter",
                "BM",
                Rule::Deprecated,
                "[/Normal] (array deprecated in 2.0)"
            ),
            (
                Some(5),
                "ArrayOfBlendModes",
                "0",
                Rule::Deprecated,
                "/Normal (deprecated in 2.0)"
            ),
        ],
        "{report}"
    );
    let report = checked_as(
        &written("1.7", &array, XrefKind::Table),
        &Hold::All(Made::Built),
    );
    assert_eq!(report.findings, [], "{report}");
}

#[test]
fn a_value_direct_where_it_must_be_indirect_or_the_reverse_is_found() {
    // An annotation's popup is an indirect reference.
    let popup = dictionary! {
        "Type" => "Annot",
        "Subtype" => "Popup",
        "Rect" => numbers(&[0, 0, 10, 10]),
    };
    let direct = changed(clean(), 4, |annot| annot.set("Popup", popup));
    let report = checked(&direct);
    assert_eq!(
        found(&report),
        [(
            Some(4),
            "AnnotText",
            "Popup",
            Rule::Indirect,
            "<< /Type /Annot /Subtype /Popup /Rect [0 0 10 10] >>"
        )],
        "{report}"
    );

    // An encrypted file's /ID is direct, in a table's trailer and in a
    // cross-reference stream; and from PDF 2.0, a cross-reference
    // stream's /Encrypt is too (pdf-issues #246), which lopdf doesn't
    // write so.
    let mut plain = Pdf::with_version("2.0");
    plain
        .objects
        .extend(clean().into_iter().map(|(n, o)| ((n, 0), o)));
    plain.max_id = 5;
    plain.trailer.set("Root", r(1));
    let id = Object::Array(vec![text("0123"), text("0123")]);
    plain.trailer.set("ID", id.clone());
    // AES-256's key isn't made from the /ID.
    let [.., (_, aes_256)] = rc4_aes_128_and_aes_256(&plain);
    for referenced in [false, true] {
        let mut pdf = plain.clone();
        if referenced {
            let at = pdf.add_object(id.clone());
            pdf.trailer.set("ID", at);
        }
        pdf.encrypt(&aes_256).unwrap();
        for bytes in lopdf_written(&pdf) {
            let read = LazyPdf::open(
                Arc::<[u8]>::from(&bytes[..]),
                Some(USER),
                &Limits::DEFAULT.reader,
            );
            let report = check(&read.unwrap(), &bytes, &Hold::All(Made::Built));
            let mut direct: Vec<_> = (found(&report).into_iter())
                .filter(|f| f.3 == Rule::Direct)
                .map(|f| (f.1, f.2, check::numberless(f.4)))
                .collect();
            direct.sort();
            let r = || "R".to_string();
            let expected = match (referenced, report.checked.contains_key("XRefStream")) {
                (false, false) => vec![],
                (true, false) => vec![("FileTrailer", "ID", r())],
                (false, true) => vec![("XRefStream", "Encrypt", r())],
                (true, true) => vec![("XRefStream", "Encrypt", r()), ("XRefStream", "ID", r())],
            };
            assert_eq!(direct, expected, "{report}");
            // A cross-reference stream's /Length, which lopdf reads into
            // its dictionary, is direct from 2.0: told from the bytes.
            let unread = (report.unchecked.keys()).any(|u| u.column == "IndirectReference");
            assert!(!unread, "{report}");
        }
    }
}

#[test]
fn a_key_the_model_doesnt_list_is_counted_by_its_kind_of_name() {
    let keys = changed(clean(), 4, |annot| {
        annot.set("SheafAnnot", 1);
        annot.set("ABCD_x", 1);
        annot.set("XXMine", 1);
    });
    // The trailer's /DocChecksum is OpenOffice's extension's key.
    let trailer = dictionary! { "DocChecksum" => "Sum" };
    let bytes = written_with("1.7", &keys, XrefKind::Table, trailer);
    let report = checked_as(&bytes, &Hold::All(Made::Built));
    assert_eq!(report.findings, [], "{report}");
    assert_eq!(
        report.unlisted,
        BTreeMap::from([
            (unlisted("AnnotText", "ABCD_x", Class::Second), 1),
            (unlisted("AnnotText", "SheafAnnot", Class::Other), 1),
            (unlisted("AnnotText", "XXMine", Class::Third), 1),
            (
                unlisted(
                    "FileTrailer",
                    "DocChecksum",
                    Class::Extension("OpenOffice".to_string())
                ),
                1
            ),
        ]),
        "{report}"
    );

    // Any other key the model doesn't list in an object sheaf built whole
    // is a finding: misspelled, or an older sheaf's private key. Merged
    // over a source's, it may be the source's, and is counted only.
    // A page's content, the plain stream table, is said to be a stream
    // of its own: a key it doesn't list is one too.
    let mut misspelled = changed(keys, 4, |annot| {
        annot.set("Contnets", text("A note"));
        annot.set("SheafPart", 1);
    });
    misspelled = changed(misspelled, 3, |page| page.set("Contents", r(6)));
    let content = Stream::new(dictionary! { "Subtype" => "Form" }, b"q Q".to_vec());
    misspelled.push((6, Object::Stream(content)));
    let bytes = written("1.7", &misspelled, XrefKind::Table);
    let report = checked_as(&bytes, &Hold::All(Made::Built));
    assert_eq!(
        found(&report),
        [
            (Some(6), "Stream", "Subtype", Rule::Unlisted, "/Form"),
            (Some(4), "AnnotText", "Contnets", Rule::Unlisted, "(A note)"),
            (Some(4), "AnnotText", "SheafPart", Rule::Unlisted, "1"),
        ],
        "{report}"
    );
    let report = checked_as(&bytes, &Hold::All(Made::Merged));
    assert_eq!(report.findings, [], "{report}");
    assert_eq!(report.unlisted.len(), 6, "{report}");
}

#[test]
fn a_special_case_false_is_found_and_one_that_cant_be_told_is_counted() {
    // A page's private data without the page's /LastModified.
    let piece = dictionary! { "Sheaf" => dictionary! {
        "LastModified" => text("D:20261024"),
        "Private" => 1,
    } };
    let objects = changed(clean(), 3, |page| page.set("PieceInfo", piece));
    // An annotation's reply type, whose SpecialCase's fn:IsMeaningful
    // isn't supported.
    let objects = changed(objects, 4, |annot| {
        annot.set("RT", "R");
        annot.set("IRT", r(4));
    });
    let report = checked(&objects);
    assert_eq!(
        found(&report),
        [
            (
                Some(3),
                "PageObject",
                "PieceInfo",
                Rule::Special,
                "<< /Sheaf << /LastModified (D:20261024) /Private 1 >> >> \
                 (fn:Eval(fn:IsPresent(LastModified)))"
            ),
            // Its Required says so too.
            (
                Some(3),
                "PageObject",
                "LastModified",
                Rule::Missing,
                "absent"
            ),
        ],
        "{report}"
    );
    let unchecked = Unchecked {
        column: "SpecialCase",
        why: "fn:IsMeaningful".to_string(),
        table: "AnnotText".to_string(),
        key: "RT".to_string(),
    };
    assert_eq!(report.unchecked.get(&unchecked), Some(&1), "{report}");
}

#[test]
fn what_the_walk_leaves_unread_the_model_never_asks() {
    for table in grammar().tables() {
        for row in &table.rows {
            let at = format!("{}.tsv line {}", table.name, row.line);
            // Deprecation is read on keys, types and values: never on links.
            let deprecated = (row.link.iter().flatten())
                .any(|l| l.wraps.iter().any(|w| w.func == WrapFn::Deprecated));
            assert!(!deprecated, "{at}: a deprecated link");
            // A tree's IndirectReference is its values', which the walk
            // doesn't read: none asks it but under an extension.
            for (ty, cell) in row.r#type.iter().zip(&row.indirect_reference) {
                if matches!(ty.item, ArlType::NameTree | ArlType::NumberTree) && cell != "FALSE" {
                    let since = predicate::since(&row.since_version);
                    assert!(matches!(since, Since::Extension(_)), "{at}: {cell}");
                }
            }
        }
    }
}

#[test]
fn beyond_leaves_out_what_a_source_has_in_merged_objects_only() {
    let at = |n: u32, made: Made, rule: Rule, value: &str| Finding {
        object: Some((n, 0)),
        made,
        path: "Root/Pages/Kids[0]/Annots[0]".to_string(),
        table: "AnnotText".to_string(),
        key: "Popup".to_string(),
        rule,
        value: value.to_string(),
    };
    let merged = at(4, Made::Merged, Rule::Type, "/Bogus");
    let built = at(5, Made::Built, Rule::Type, "/Bogus");
    let renumbered = at(4, Made::Merged, Rule::Type, "<< /Kids [3 0 R 14 0 R] >>");
    let other = at(4, Made::Merged, Rule::Type, "/Other");
    let missing = at(4, Made::Merged, Rule::Missing, "/Bogus");
    let written = Report {
        findings: vec![
            merged.clone(),
            built.clone(),
            renumbered.clone(),
            other.clone(),
            missing.clone(),
        ],
        ..Report::default()
    };
    let one = Report {
        findings: vec![at(9, Made::Merged, Rule::Type, "/Bogus")],
        ..Report::default()
    };
    let two = Report {
        findings: vec![at(9, Made::Built, Rule::Type, "<< /Kids [30 0 R 7 0 R] >>")],
        ..Report::default()
    };
    assert_eq!(
        written.beyond(&[]),
        [&merged, &built, &renumbered, &other, &missing]
    );
    assert_eq!(
        written.beyond(&[&one]),
        [&built, &renumbered, &other, &missing],
        "a built object's finding is never carried"
    );
    assert_eq!(
        written.beyond(&[&one, &two]),
        [&built, &other, &missing],
        "a reference's numbers aside"
    );

    for (value, numberless) in [
        ("3 0 R", "R"),
        ("<< /Kids [3 0 R 14 0 R] >>", "<< /Kids [R R] >>"),
        (
            "its /Kids[0] is 12 0 R, no object",
            "its /Kids[0] is R, no object",
        ),
        ("[1 2 3]", "[1 2 3]"),
        ("/A1 0 R", "/A1 0 R"),
        ("1 0 Rx", "1 0 Rx"),
        ("(é 2 0 R)", "(é R)"),
    ] {
        assert_eq!(check::numberless(value), numberless, "{value}");
    }
}

/// The carried rule matches a merged object's finding against every
/// object of the sources, not the one it was emitted over (backlog B46):
/// a page sheaf wrote without its `/MediaBox` is carried when any page of
/// a source lacks one. Matching each object to its own source is B46's.
#[test]
fn a_merged_objects_finding_is_carried_from_any_object_of_a_source() {
    let missing = |n: u32, made: Made| Finding {
        object: Some((n, 0)),
        made,
        path: format!("Root/Pages/Kids[{n}]"),
        table: "PageObject".to_string(),
        key: "MediaBox".to_string(),
        rule: Rule::Missing,
        value: "absent".to_string(),
    };
    let written = Report {
        findings: vec![missing(3, Made::Merged), missing(4, Made::Built)],
        ..Report::default()
    };
    let another_page = Report {
        findings: vec![missing(9, Made::Merged)],
        ..Report::default()
    };
    assert_eq!(written.beyond(&[&another_page]), [&missing(4, Made::Built)]);
}

#[test]
fn a_source_carries_a_key_too_new_whatever_its_value_and_a_trailers_form() {
    let at = |object: Option<u32>, made: Made, (table, key): (&str, &str), value: &str| Finding {
        object: object.map(|n| (n, 0)),
        made,
        path: "-".to_string(),
        table: table.to_string(),
        key: key.to_string(),
        rule: Rule::TooNew,
        value: value.to_string(),
    };
    let info = at(
        Some(3),
        Made::Merged,
        ("PageObject", "PieceInfo"),
        "<< … 9 >>",
    );
    let built = at(
        Some(8),
        Made::Built,
        ("PageObject", "PieceInfo"),
        "<< … 9 >>",
    );
    let size = at(
        Some(20),
        Made::Merged,
        ("XRefStream", "Size"),
        "21 (since 1.5)",
    );
    let index = at(
        Some(20),
        Made::Merged,
        ("ArrayOfXRefIndexIntegers", "2"),
        "7",
    );
    let written = Report {
        findings: vec![info.clone(), built.clone(), size.clone(), index.clone()],
        trailer: Some((20, 0)),
        ..Report::default()
    };
    let page = at(
        Some(3),
        Made::Merged,
        ("PageObject", "PieceInfo"),
        "<< … 4 >>",
    );
    let pages = Report {
        findings: vec![
            page.clone(),
            at(Some(3), Made::Merged, ("XRefStream", "W"), ""),
        ],
        trailer: Some((12, 0)),
        ..Report::default()
    };
    assert_eq!(
        written.beyond(&[&pages]),
        [&built, &size, &index],
        "a page's key, whatever its value; and not the trailer's, from no trailer"
    );
    let stream = Report {
        findings: vec![page, at(Some(12), Made::Merged, ("XRefStream", "W"), "")],
        trailer: Some((12, 0)),
        ..Report::default()
    };
    assert_eq!(
        written.beyond(&[&stream]),
        [&built],
        "the trailer's form, which the source's trailer is too"
    );
    let table = Report {
        findings: vec![at(None, Made::Merged, ("FileTrailer", "XRefStm"), "9")],
        ..Report::default()
    };
    assert_eq!(written.beyond(&[&table]), [&info, &built]);
}

#[test]
fn a_file_is_checked_at_the_version_asked() {
    // Before 1.3 a page's /PieceInfo is too new; from 1.3 it asks for the
    // page's /LastModified too.
    let piece = changed(clean(), 3, |page| {
        page.set("PieceInfo", dictionary! { "Mine" => dictionary! {} })
    });
    let bytes = written("1.2", &piece, XrefKind::Table);
    let pdf = open(&bytes);
    assert_eq!(file_version(&pdf), PdfVersion::new(1, 2));
    let as_written = check(&pdf, &bytes, &Hold::All(Made::Built));
    let at_1_4 = check_at(&pdf, &bytes, &Hold::All(Made::Built), PdfVersion::new(1, 4));
    let page = |report: &Report| {
        (report.findings.iter())
            .filter(|f| f.table == "PageObject")
            .map(|f| (f.key.clone(), f.rule))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        page(&as_written),
        [
            ("PieceInfo".to_string(), Rule::TooNew),
            ("PieceInfo".to_string(), Rule::Special)
        ],
        "{as_written}"
    );
    assert_eq!(
        page(&at_1_4),
        [
            ("PieceInfo".to_string(), Rule::Special),
            ("LastModified".to_string(), Rule::Missing)
        ],
        "{at_1_4}"
    );
}

/// `text` as an allowances file, read.
fn allowances(text: &str) -> anyhow::Result<Allowances> {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "sheaf-arlington-allowed-{}-{n}.tsv",
        std::process::id()
    ));
    std::fs::write(&path, text).unwrap();
    let read = Allowances::read(&path);
    std::fs::remove_file(&path).unwrap();
    read
}

#[test]
fn allowances_each_name_a_backlog_entry_and_a_reason() {
    let header = "# Held for the test.\n";
    let rows = "FieldTx\tDA\tmissing\t*\t*\tB40\tthe form's /DA, which the model doesn't read\n\
                -\t-\tunreached\t*\t*\tF24\ta reason\n\
                FieldTx\tFT\tmissing\tforms/a.pdf\t*\tB41\tone file's\n\
                FieldTx\tV\ttype\t*\t2.0\tB42\t2.0 files' and later\n";
    let held = allowances(&format!("{header}{rows}")).unwrap();
    let v1_7 = PdfVersion::new(1, 7);
    let missing = finding(
        6,
        "Root/AcroForm/Fields[0]",
        ("FieldTx", "DA"),
        Rule::Missing,
        "absent",
    );
    let allowed = held
        .allows(&missing, v1_7, None)
        .map(|a| (a.entry.as_str(), a.line));
    assert_eq!(allowed, Some(("B40", 2)));
    let typed = Finding {
        rule: Rule::Type,
        ..missing.clone()
    };
    assert_eq!(held.allows(&typed, v1_7, None), None);
    let ft = Finding {
        key: "FT".to_string(),
        ..missing.clone()
    };
    assert_eq!(held.allows(&ft, v1_7, None), None, "in no corpus file");
    assert_eq!(
        held.allows(&ft, v1_7, Some("forms/b.pdf")),
        None,
        "in another"
    );
    let allowed = held.allows(&ft, v1_7, Some("forms/a.pdf")).map(|a| a.line);
    assert_eq!(allowed, Some(4));
    let v = Finding {
        key: "V".to_string(),
        ..typed
    };
    assert_eq!(held.allows(&v, v1_7, None), None, "in a 1.7 file");
    for version in [PdfVersion::new(2, 0), PdfVersion::new(2, 1)] {
        let allowed = held.allows(&v, version, None).map(|a| a.line);
        assert_eq!(allowed, Some(5), "in a {version} file");
    }
    let unused: Vec<_> = held.unused().iter().map(|a| a.line).collect();
    assert_eq!(unused, [3]);

    for (rows, why) in [
        (
            "FieldTx\tDA\tmissing\t*\t*\t\ta reason\n",
            "line 2: the entry \"\"",
        ),
        (
            "FieldTx\tDA\tmissing\t*\t*\tX40\ta reason\n",
            "line 2: the entry \"X40\"",
        ),
        ("FieldTx\tDA\tmissing\t*\t*\tB40\t\n", "line 2: no reason"),
        ("FieldTx\tDA\tmissing\t\t*\tB40\ta\n", "line 2: no file"),
        (
            "FieldTx\tDA\tmissing\t*\t2\tB40\ta\n",
            "line 2: \"2\" isn't a version",
        ),
        ("FieldTx\tDA\tmissing\t*\tB40\ta\n", "line 2: 6 fields"),
        (
            "Nope\tDA\tmissing\t*\t*\tB40\ta reason\n",
            "line 2: \"Nope\" isn't a table",
        ),
        (
            "FieldTx\tDA\tbogus\t*\t*\tB40\ta reason\n",
            "line 2: \"bogus\" isn't a rule",
        ),
        (
            "FieldTx\tDA\tmissing\t*\t*\tB40\ta\nFieldTx\tDA\tmissing\t*\t*\tB41\tb\n",
            "line 3: FieldTx/DA missing in * from * is on line 2 already",
        ),
        (
            "FieldTx\tDA\tmissing\t*\t*\tB40\ta\n# late\n",
            "line 3: a # line among the rows",
        ),
    ] {
        let Err(e) = allowances(&format!("{header}{rows}")) else {
            panic!("{rows:?} read");
        };
        assert!(format!("{e:#}").contains(why), "{rows:?}: {e:#}");
    }
    let Err(e) = allowances(rows) else {
        panic!("no header, read");
    };
    assert!(format!("{e:#}").contains("line 1: no # header"), "{e:#}");
}
