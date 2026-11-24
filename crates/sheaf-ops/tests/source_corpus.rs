//! Every corpus file PDFium opens opens as a source, and every page's
//! size, as the source's page table reads it, is the size PDFium gives it
//! (M10.5 slice 3): the app opens a file through its source, the layout
//! takes its sizes from the table, and PDFium renders the pages by index
//! (a base engine's or an overlay's). Skips when there's no corpus (see
//! `sheaf_doc::forms::corpus`).
//!
//! Over `forms/` every time; over the scans, signatures and conformance
//! suites and the pool sample in `just tier-pages`, where the files that
//! PDFium numbers otherwise are held by name (M18 slice 21).
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;

use sheaf_doc::Store;
use sheaf_doc::forms::corpus::{Corpus, Entry};
use sheaf_doc::geom::Geom;
use sheaf_edit::PageSize;
use sheaf_render::PageIndex;
use sheaf_render::Renderer;
use sheaf_render::pdfium::PdfiumEngine;

/// Each page's size as the page table has it.
fn table_sizes(path: &Path) -> Result<Vec<PageSize>, String> {
    let (src, _) = (Store::new().read_file(path, None)).map_err(|e| format!("{e:#}"))?;
    Ok((0..src.page_count()).map(|i| src.page_size(i)).collect())
}

/// Each page's size as PDFium has it; None when PDFium doesn't open the
/// file.
fn pdfium_sizes(path: &Path) -> Option<Vec<Result<PageSize, String>>> {
    let bytes = std::fs::read(path).ok()?;
    let engine = PdfiumEngine::open_bytes(bytes.into(), None).ok()?;
    Some(
        (0..engine.page_count())
            .map(|i| engine.page_size(PageIndex(i)).map_err(|e| format!("{e:#}")))
            .collect(),
    )
}

#[test]
fn every_corpus_page_s_geom_is_pdfium_s_size() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let entries = corpus.forms().unwrap();
    // The sources open on other threads, PDFium on this one: sheaf holds
    // one lock across every use of it.
    let queue = Mutex::new(entries.iter().enumerate());
    let tables = Mutex::new(BTreeMap::new());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(16));
    let mut engines = Vec::new();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let Some((i, entry)) = queue.lock().unwrap().next() else {
                        break;
                    };
                    let sizes = table_sizes(&corpus.path(entry));
                    tables.lock().unwrap().insert(i, sizes);
                }
            });
        }
        for entry in &entries {
            engines.push(pdfium_sizes(&corpus.path(entry)));
        }
    });
    let tables = tables.into_inner().unwrap();

    let (mut opened, mut pages) = (0, 0);
    let mut differ: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for ((entry, engine), table) in entries.iter().zip(engines).zip(tables.into_values()) {
        let Some(engine) = engine else {
            continue;
        };
        opened += 1;
        let table = match table {
            Ok(table) => table,
            Err(e) => {
                differ.insert(
                    &entry.path,
                    vec![format!("PDFium opens it, the source doesn't: {e}")],
                );
                continue;
            }
        };
        let mut found = Vec::new();
        if table.len() != engine.len() {
            found.push(format!(
                "{} pages in the table, {} in PDFium",
                table.len(),
                engine.len()
            ));
        }
        for (page, (table, pdfium)) in table.into_iter().zip(engine).enumerate() {
            pages += 1;
            if pdfium.as_ref() != Ok(&table) {
                found.push(format!(
                    "page {page}: {table:?} in the table, {pdfium:?} in PDFium"
                ));
            }
        }
        if !found.is_empty() {
            differ.insert(&entry.path, found);
        }
    }
    println!(
        "{} files, {opened} PDFium opens, {pages} pages compared, {} files differ",
        entries.len(),
        differ.len()
    );
    for (path, found) in &differ {
        println!(
            "{path}: {} differences: {}",
            found.len(),
            found[..found.len().min(3)].join("; ")
        );
    }
    assert!(opened > 0, "PDFium opened nothing");
    assert!(differ.is_empty(), "{} files differ", differ.len());
}

/// A page as the page table has it: its size, and whether it holds
/// nothing at all, as the table's stand-in for a place where PDFium loads
/// no page does.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Listed {
    size: PageSize,
    empty: bool,
}

/// Each page as the page table has it.
fn table_pages(path: &Path) -> Result<Vec<Listed>, String> {
    let (src, _) = (Store::new().read_file(path, None)).map_err(|e| format!("{e:#}"))?;
    let listed = (src.pages().iter().enumerate()).map(|(i, page)| Listed {
        size: src.page_size(i as u32),
        empty: page.parent.is_none()
            && page.resources.is_none()
            && page.contents.is_empty()
            && page.annots.is_empty()
            && page.struct_parents.is_none()
            && page.geom == Geom::read(None, None, 0),
    });
    Ok(listed.collect())
}

/// A file as PDFium and the page table read it.
struct Both {
    /// Each page's size as PDFium has it, an error where it loads none
    /// there; None when PDFium doesn't open the file.
    pdfium: Option<Vec<Result<PageSize, String>>>,
    table: Result<Vec<Listed>, String>,
}

/// Each of `paths` read by the page table, on other threads, and by
/// PDFium, on this one, in their order: None for one not in the corpus.
fn read_both(corpus: &Corpus, paths: &[String]) -> Vec<Option<Both>> {
    let at = |path: &str| {
        corpus.path(&Entry {
            path: path.to_string(),
            ..Entry::default()
        })
    };
    let queue = Mutex::new(paths.iter().enumerate());
    let tables = Mutex::new(BTreeMap::new());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(16));
    let mut engines = Vec::new();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let Some((i, path)) = queue.lock().unwrap().next() else {
                        break;
                    };
                    let path = at(path);
                    let table = path.exists().then(|| table_pages(&path));
                    tables.lock().unwrap().insert(i, table);
                }
            });
        }
        for path in paths {
            engines.push(pdfium_sizes(&at(path)));
        }
    });
    let tables = tables.into_inner().unwrap();
    (engines.into_iter().zip(tables.into_values()))
        .map(|(pdfium, table)| {
            Some(Both {
                pdfium,
                table: table?,
            })
        })
        .collect()
}

/// A file the table numbers apart from PDFium, held to PDFium's own
/// answer: how many pages it counts and which of them it loads, and which
/// of those the table has.
struct Held {
    path: &'static str,
    /// Whether PDFium loads each of its pages: its count is their number.
    loads: &'static [bool],
    /// The PDFium page each of the table's is, in order; none where the
    /// source doesn't open, having no pages. A page PDFium loads is the
    /// table's at its size, and one it doesn't is the table's stand-in.
    table: &'static [usize],
    why: &'static str,
}

/// Six files whose page trees the walk before M18 slice 21 read otherwise
/// than PDFium does, by path, where the corpus has them.
const HELD: [Held; 6] = [
    Held {
        path: "parsing/pdfium/resources/bad_page_type.pdf",
        loads: &[true, false],
        table: &[0, 1],
        why: "its second leaf is a /Template: a place with no page",
    },
    Held {
        path: "parsing/pdfium/generated/bad_page_type.pdf",
        loads: &[true, false],
        table: &[0, 1],
        why: "its second leaf is a /Template: a place with no page",
    },
    Held {
        path: "parsing/jhove/Test_Corpus/T02-03_002_page-object-wrong-type.pdf",
        loads: &[false],
        table: &[0],
        why: "its one leaf is a /Font: a place with no page",
    },
    Held {
        path: "parsing/pdfium/generated/javascript/bug_1314658.pdf",
        loads: &[true],
        table: &[],
        why: "its one kid is an object that doesn't parse, which PDFium reads as a \
              dictionary and loads as a page: the reader doesn't, so the walk \
              counts no page there, and the file has none",
    },
    Held {
        path: "parsing/qpdf/qtest/qpdf/direct-pages.pdf",
        loads: &[true, true],
        table: &[],
        why: "both its pages are written in place, with no object number: the \
              table leaves them out, and has none",
    },
    Held {
        path: "signatures/pyhanko/pdf/page-tree-direct-kid.pdf",
        loads: &[true, true],
        table: &[0],
        why: "its second page is written in place, with no object number: the \
              table leaves it out",
    },
];

/// Whether the table's page `listed` is PDFium's `pdfium`: at its size
/// where PDFium loads a page, and the table's stand-in where it loads
/// none.
fn agrees(listed: &Listed, pdfium: &Result<PageSize, String>) -> bool {
    match pdfium {
        Ok(size) => *size == listed.size,
        Err(_) => listed.empty,
    }
}

impl Held {
    /// How `both` departs from what's held: nothing when it doesn't.
    fn departures(&self, both: &Both) -> Vec<String> {
        let Some(pdfium) = &both.pdfium else {
            return vec!["PDFium doesn't open it".to_string()];
        };
        let mut found = Vec::new();
        let loads: Vec<bool> = pdfium.iter().map(Result::is_ok).collect();
        if loads != self.loads {
            found.push(format!("PDFium loads {loads:?}, held {:?}", self.loads));
        }
        let table = match &both.table {
            Ok(table) => table,
            Err(e) if self.table.is_empty() && e.ends_with("the file has no pages") => {
                return found;
            }
            Err(e) => {
                found.push(format!("the source doesn't open: {e}"));
                return found;
            }
        };
        if table.len() != self.table.len() {
            found.push(format!(
                "{} pages in the table, held {}",
                table.len(),
                self.table.len()
            ));
        }
        for (page, (listed, &at)) in table.iter().zip(self.table).enumerate() {
            match pdfium.get(at) {
                Some(pdfium) if agrees(listed, pdfium) => {}
                pdfium => found.push(format!(
                    "page {page}: {listed:?} in the table, PDFium's page {at}: {pdfium:?}"
                )),
            }
        }
        found
    }
}

/// The files whose root's `/Count` is more than their trees list: PDFium
/// counts by it, and loads no page past those the tree lists, where the
/// table ends.
const PAST_THE_TREE: &[&str] = &[];

/// The files PDFium opens and the source doesn't, each with the end of
/// the source's error: a header the reader refuses, pages in object
/// streams that don't read, a `/Kids` cycle PDFium goes on past, and
/// encryption the reader can't read.
const UNOPENED: &[(&str, &str)] = &[
    (
        "conformance/verapdf/PDF_A-1b/6.1 File structure/6.1.2 File header/veraPDF test suite 6-1-2-t01-fail-b.pdf",
        "invalid PDF file header",
    ),
    (
        "signatures/pyhanko/pdf/broken-objstream1.pdf",
        "the file has no pages",
    ),
    (
        "signatures/pyhanko/pdf/broken-objstream2.pdf",
        "the file has no pages",
    ),
    (
        "signatures/pyhanko/pdf/broken-objstream3.pdf",
        "the file has no pages",
    ),
    (
        "signatures/pyhanko/pdf/broken-objstream6.pdf",
        "the file has no pages",
    ),
    (
        "signatures/pyhanko/pdf/circular-page-tree.pdf",
        "the page tree lists a node inside itself",
    ),
    (
        "signatures/pyhanko/pdf/embedded-encrypted-nocf.pdf",
        "the password is wrong, or the file needs one",
    ),
    (
        "signatures/pyhanko/pdf/malformed-encrypt-dict2.pdf",
        "the password is wrong, or the file needs one",
    ),
    (
        "signatures/pyhanko/pdf/minimal-illegal-header.pdf",
        "invalid PDF file header",
    ),
];

/// Every file of the scans, signatures and conformance manifests and the
/// pool's sample, and the files [`HELD`] names, each once, in the order of
/// their paths.
fn other_files(corpus: &Corpus) -> Vec<String> {
    let list = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/pool-sample.txt");
    let pool = std::fs::read_to_string(list).expect("bench/pool-sample.txt");
    let pool = (pool.lines().map(str::trim))
        .filter(|l| !l.is_empty())
        .map(str::to_string);
    let manifests = [corpus.scans(), corpus.signatures(), corpus.conformance()];
    let listed = (manifests.into_iter())
        .flat_map(|rows| rows.expect("the corpus's manifests"))
        .map(|e| e.path);
    let held = HELD.iter().map(|held| held.path.to_string());
    let files: BTreeSet<String> = listed.chain(pool).chain(held).collect();
    files.into_iter().collect()
}

/// Every file of the scans, signatures and conformance suites and the
/// pool sample that PDFium opens opens as a source but those [`UNOPENED`]
/// names, and has PDFium's count of pages, each PDFium's size where it
/// loads one and the table's stand-in where it doesn't; but for the files
/// [`PAST_THE_TREE`] names, whose pages are PDFium's as far as their trees
/// list them, and those [`HELD`] names, each held to PDFium's own answer.
#[test]
#[ignore = "the scans, signatures and conformance suites and the pool sample against PDFium: `just tier-pages`"]
fn every_page_of_the_other_suites_is_pdfium_s() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let started = std::time::Instant::now();
    let files = other_files(&corpus);
    let read = read_both(&corpus, &files);

    let (mut absent, mut opened, mut pages) = (Vec::new(), 0, 0);
    let mut unopened: BTreeMap<&str, String> = BTreeMap::new();
    let mut past: Vec<&str> = Vec::new();
    let mut held = 0;
    let mut differ: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (path, both) in files.iter().zip(read) {
        let Some(both) = both else {
            absent.push(path);
            continue;
        };
        if let Some(rule) = HELD.iter().find(|held| held.path == path) {
            held += 1;
            let found = rule.departures(&both);
            if !found.is_empty() {
                differ.insert(path, found);
            }
            continue;
        }
        let Some(pdfium) = both.pdfium else {
            continue;
        };
        let table = match both.table {
            Ok(table) => table,
            Err(e) => {
                unopened.insert(path, e);
                continue;
            }
        };
        opened += 1;
        let mut found = Vec::new();
        for (page, (listed, pdfium)) in table.iter().zip(&pdfium).enumerate() {
            pages += 1;
            if !agrees(listed, pdfium) {
                found.push(format!(
                    "page {page}: {listed:?} in the table, {pdfium:?} in PDFium"
                ));
            }
        }
        let beyond = pdfium.get(table.len()..).unwrap_or_default();
        if found.is_empty() && table.len() < pdfium.len() && beyond.iter().all(Result::is_err) {
            past.push(path);
            continue;
        }
        if table.len() != pdfium.len() {
            found.insert(
                0,
                format!(
                    "{} pages in the table, {} in PDFium",
                    table.len(),
                    pdfium.len()
                ),
            );
        }
        if !found.is_empty() {
            differ.insert(path, found);
        }
    }
    println!(
        "{} files ({} not in the corpus), {opened} opened by both, {pages} pages \
         compared, {} PDFium opens and the source doesn't, {} past the tree, {held} held, \
         {} differ, in {:.0} s",
        files.len(),
        absent.len(),
        unopened.len(),
        past.len(),
        differ.len(),
        started.elapsed().as_secs_f64()
    );
    for path in &absent {
        println!("not in the corpus: {path:?}");
    }
    for (path, e) in &unopened {
        println!("PDFium opens it, the source doesn't: {path}: {e}");
    }
    for path in &past {
        println!("past the tree: {path}");
    }
    for rule in &HELD {
        println!("held: {}: {}", rule.path, rule.why);
    }
    for (path, found) in &differ {
        println!(
            "{path}: {} differences: {}",
            found.len(),
            found[..found.len().min(3)].join("; ")
        );
    }
    assert!(opened > 0, "PDFium and the source opened nothing");
    let held_unopened: Vec<&str> = UNOPENED.iter().map(|(path, _)| *path).collect();
    assert_eq!(
        unopened.keys().copied().collect::<Vec<_>>(),
        held_unopened,
        "the files PDFium opens and the source doesn't"
    );
    for (path, why) in UNOPENED {
        assert!(unopened[path].ends_with(why), "{path}: {}", unopened[path]);
    }
    assert_eq!(
        past, PAST_THE_TREE,
        "the files whose /Count is past the tree"
    );
    assert!(differ.is_empty(), "{} files differ", differ.len());
}
