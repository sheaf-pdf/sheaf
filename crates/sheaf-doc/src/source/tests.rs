//! The page table and what a source says of itself: pages numbered and
//! sized as PDFium numbers and sizes them, a hostile tree a typed error
//! within the caps, the conformance claims in its XMP, and how it's
//! encrypted.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::pdf::{Object, ObjectId, Value, deflate};
use sheaf_edit::{PageSize, Rotation};

use super::lazy_tests::{QUICK, catalog, plaintext, stream, with_table};
use super::{Conformance, Limits, OpenError, PageTreeError, PageTreeLimits, PdfA, PdfUa};
use crate::geom::Rect;
use crate::serialize::fixture::{
    USER, attachment_only, encrypt_dict_for, encrypted, rc4_aes_128_and_aes_256,
};
use crate::store::{BlobKind, Src, Store};

fn object(number: u32, body: &str) -> (u32, Vec<u8>) {
    (number, body.as_bytes().to_vec())
}

/// A file of `objects`, the first the catalog, opened as a source under
/// `limits`.
fn open_with(objects: &[(u32, Vec<u8>)], limits: Limits) -> Result<Src, OpenError> {
    let kind = BlobKind::file(Path::new("t.pdf"), None);
    Store::with_limits(limits).open(with_table(objects).into(), kind)
}

fn open(objects: &[(u32, Vec<u8>)]) -> Result<Src, OpenError> {
    open_with(objects, Limits::DEFAULT)
}

fn tree_error(result: Result<Src, OpenError>) -> PageTreeError {
    match result {
        Err(OpenError::PageTree(e)) => e,
        Err(e) => panic!("not a page tree error: {e}"),
        Ok(src) => panic!("opened, {} pages", src.page_count()),
    }
}

fn ids(src: &Src) -> Vec<(ObjectId, u32)> {
    (0..src.page_count())
        .map(|i| src.page(i).unwrap())
        .map(|p| (p.id, p.repeat))
        .collect()
}

fn size(width: f32, height: f32) -> PageSize {
    PageSize { width, height }
}

#[test]
fn cyclic_kids_are_a_typed_error() {
    let started = Instant::now();
    let page = object(4, "<< /Type /Page /Parent 2 0 R >>");
    // 2 lists 3, which lists 2.
    let cycle = [
        catalog(),
        object(2, "<< /Type /Pages /Kids [4 0 R 3 0 R] /Count 2 >>"),
        object(3, "<< /Type /Pages /Parent 2 0 R /Kids [2 0 R] /Count 1 >>"),
        page.clone(),
    ];
    assert_eq!(tree_error(open(&cycle)), PageTreeError::Cycle);
    // The root lists itself.
    let itself = [
        catalog(),
        object(2, "<< /Type /Pages /Kids [4 0 R 2 0 R] /Count 2 >>"),
        page,
    ];
    assert_eq!(tree_error(open(&itself)), PageTreeError::Cycle);
    assert!(started.elapsed() < QUICK);
}

#[test]
fn a_parent_loop_is_a_typed_error() {
    // The page's /Parent is 3, 3's is 4, and 4's is 3 again.
    let objects = [
        catalog(),
        object(2, "<< /Type /Pages /Kids [5 0 R] /Count 1 >>"),
        object(3, "<< /Type /Pages /Parent 4 0 R /Kids [5 0 R] >>"),
        object(4, "<< /Type /Pages /Parent 3 0 R /Kids [3 0 R] >>"),
        object(5, "<< /Type /Page /Parent 3 0 R >>"),
    ];
    assert_eq!(tree_error(open(&objects)), PageTreeError::ParentLoop);
    // A page that is its own /Parent.
    let objects = [
        catalog(),
        object(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        object(3, "<< /Type /Page /Parent 3 0 R >>"),
    ];
    assert_eq!(tree_error(open(&objects)), PageTreeError::ParentLoop);
}

/// A chain of `depth` nodes from the root (2) down, each listing the next
/// and naming the one above as its `/Parent`, and a page under the last.
fn chain(depth: u32) -> Vec<(u32, Vec<u8>)> {
    let mut objects = vec![catalog()];
    for n in 2..2 + depth {
        let parent = if n == 2 {
            String::new()
        } else {
            format!("/Parent {} 0 R ", n - 1)
        };
        objects.push(object(
            n,
            &format!("<< /Type /Pages {parent}/Kids [{} 0 R] /Count 1 >>", n + 1),
        ));
    }
    let last = 1 + depth;
    objects.push(object(
        last + 1,
        &format!("<< /Type /Page /Parent {last} 0 R /MediaBox [0 0 100 200] >>"),
    ));
    objects
}

#[test]
fn a_10000_deep_page_tree_is_a_typed_error() {
    let started = Instant::now();
    assert_eq!(tree_error(open(&chain(10_000))), PageTreeError::TooDeep);
    assert!(started.elapsed() < QUICK, "{:?}", started.elapsed());
    // PDFium's 1,024 levels open, and the page inherits down its 1,024
    // parents.
    let deepest = open(&chain(1024)).unwrap();
    assert_eq!(deepest.page_size(0), size(100.0, 200.0));
    assert_eq!(deepest.page(0).unwrap().parents().count(), 1024);
    assert_eq!(tree_error(open(&chain(1025))), PageTreeError::TooDeep);

    // A shallow tree whose page names a /Parent 10,000 nodes from the top.
    let mut objects = vec![
        catalog(),
        object(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        object(3, "<< /Type /Page /Parent 4 0 R >>"),
    ];
    for n in 4..10_004 {
        objects.push(object(
            n,
            &format!("<< /Type /Pages /Parent {} 0 R >>", n + 1),
        ));
    }
    let started = Instant::now();
    assert_eq!(tree_error(open(&objects)), PageTreeError::TooDeep);
    assert!(started.elapsed() < QUICK, "{:?}", started.elapsed());
}

/// What pdf-semantics is given is what sheaf reads with: its defaults
/// under sheaf's caps on the page tree and on what a stream decodes to.
#[test]
fn the_familys_limits_are_sheafs() {
    use crate::pdf::semantics as family;
    // The page tree's defaults are the same caps already.
    let mut want = family::Limits::DEFAULT;
    want.xmp_bytes = Limits::DEFAULT.reader.max_decompressed;
    assert_eq!(Limits::DEFAULT.semantics(), want);
    let limits = Limits {
        reader: crate::pdf::ReadLimits {
            max_decompressed: 3002,
            ..Limits::DEFAULT.reader
        },
        page_tree: PageTreeLimits { depth: 5, pages: 7 },
        ..Limits::DEFAULT
    };
    want.page_depth = 5;
    want.pages = 7;
    want.xmp_bytes = 3002;
    assert_eq!(limits.semantics(), want);
}

#[test]
fn too_many_pages_or_none_are_typed_errors() {
    let limits = Limits {
        page_tree: PageTreeLimits {
            pages: 2,
            ..Limits::DEFAULT.page_tree
        },
        ..Limits::DEFAULT
    };
    let three = [
        catalog(),
        object(2, "<< /Type /Pages /Kids [3 0 R 3 0 R 3 0 R] /Count 3 >>"),
        object(3, "<< /Type /Page /Parent 2 0 R >>"),
    ];
    assert_eq!(
        open_with(&three, limits).unwrap_err().to_string(),
        "the page tree lists too many pages"
    );
    assert_eq!(open(&three).unwrap().page_count(), 3);
    // A node listing the next twice, 20 deep: a million visits the walk
    // stops at its cap.
    let mut objects = vec![catalog()];
    for n in 2..22 {
        objects.push(object(
            n,
            &format!("<< /Type /Pages /Kids [{0} 0 R {0} 0 R] >>", n + 1),
        ));
    }
    objects.push(object(22, "<< /Type /Pages /Kids [] >>"));
    let started = Instant::now();
    assert_eq!(
        tree_error(open_with(&objects, limits)),
        PageTreeError::TooManyPages
    );
    assert!(started.elapsed() < QUICK);

    let empty = [catalog(), object(2, "<< /Type /Pages /Kids [] /Count 0 >>")];
    assert_eq!(tree_error(open(&empty)), PageTreeError::NoPages);
    let no_tree = [object(1, "<< /Type /Catalog >>")];
    assert_eq!(tree_error(open(&no_tree)), PageTreeError::NoPages);
}

#[test]
fn a_page_listed_twice_is_two_pages() {
    let objects = [
        catalog(),
        object(
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R 3 0 R 5 0 R] /Count 5 >>",
        ),
        object(3, "<< /Type /Page /Parent 2 0 R >>"),
        object(4, "<< /Type /Page /Parent 2 0 R >>"),
        // A node listed twice lists its page twice, as PDFium walks it.
        object(
            5,
            "<< /Type /Pages /Parent 2 0 R /Kids [6 0 R 6 0 R] /Count 2 >>",
        ),
        object(6, "<< /Type /Page /Parent 5 0 R >>"),
    ];
    let src = open(&objects).unwrap();
    assert_eq!(
        ids(&src),
        [
            (ObjectId::new(3, 0), 0),
            (ObjectId::new(4, 0), 0),
            (ObjectId::new(3, 0), 1),
            (ObjectId::new(6, 0), 0),
            (ObjectId::new(6, 0), 1)
        ]
    );
    // A kid with /Kids is a node whatever its /Type, and any other a page,
    // but where its /Type isn't /Page: object 4, /Type /Pages without
    // /Kids, is a place where PDFium loads no page, Letter, though it names
    // a media box. A kid that names no dictionary keeps its place as a
    // Letter page, as PDFium numbers it. One written in place, which has
    // no object to be, takes the fourth of the root's /Count, where the
    // walk ends, and is left out: three pages, 5 0 R unlisted.
    let objects = [
        catalog(),
        object(
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R 9 0 R << /Type /Page >> 5 0 R] /Count 4 >>",
        ),
        object(3, "<< /Type /Page /Kids [4 0 R] >>"),
        object(4, "<< /Type /Pages /Parent 2 0 R /MediaBox [0 0 10 10] >>"),
        object(5, "[1 2 3]"),
    ];
    let src = open(&objects).unwrap();
    assert_eq!(
        ids(&src),
        [
            (ObjectId::new(4, 0), 0),
            (ObjectId::new(4, 0), 1),
            (ObjectId::new(9, 0), 0)
        ]
    );
    assert_eq!(src.page_size(1), size(612.0, 792.0));
    assert_eq!(src.page_size(2), size(612.0, 792.0));
    assert!(src.page(1).unwrap().parent.is_none());
    // A root without /Kids is the one page.
    let one = [
        catalog(),
        object(2, "<< /Type /Page /MediaBox [0 0 10 20] >>"),
    ];
    let src = open(&one).unwrap();
    assert_eq!(ids(&src), [(ObjectId::new(2, 0), 0)]);
    assert_eq!(src.page_size(0), size(10.0, 20.0));
    // Past the last page, Letter.
    assert_eq!(src.page_size(1), size(612.0, 792.0));
}

/// The two rules the second fixture of
/// [`a_page_listed_twice_is_two_pages`] holds together, apart.
#[test]
fn a_leaf_of_another_type_and_a_kid_in_place_are_two_rules() {
    // A leaf whose /Type is something other than /Page is a place where
    // PDFium loads no page: Letter, and holding nothing, though it names a
    // box, contents and a /Parent. A leaf with no /Type is a page.
    let objects = [
        catalog(),
        object(2, "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>"),
        object(3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 200] >>"),
        object(
            4,
            "<< /Type /Template /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 6 0 R >>",
        ),
        object(5, "<< /Parent 2 0 R /MediaBox [0 0 10 20] >>"),
    ];
    let src = open(&objects).unwrap();
    let all = [3, 4, 5].map(|n| (ObjectId::new(n, 0), 0));
    assert_eq!(ids(&src), all);
    assert_eq!(src.page_size(0), size(100.0, 200.0));
    assert_eq!(src.page_size(1), size(612.0, 792.0));
    let place = src.page(1).unwrap();
    assert!(place.parent.is_none() && place.contents.is_empty());
    assert_eq!(src.page_size(2), size(10.0, 20.0));

    // A kid written in place takes its place in PDFium's numbering, the
    // second of three, but has no object number, and is left out: the page
    // after it is the table's second, which PDFium numbers third.
    let objects = [
        catalog(),
        object(
            2,
            "<< /Type /Pages /Kids [3 0 R << /Type /Page /MediaBox [0 0 50 50] >> 4 0 R] \
             /Count 3 >>",
        ),
        object(3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 200] >>"),
        object(4, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 20] >>"),
    ];
    let src = open(&objects).unwrap();
    let all = [3, 4].map(|n| (ObjectId::new(n, 0), 0));
    assert_eq!(ids(&src), all);
    assert_eq!(src.page_size(1), size(10.0, 20.0));
}

#[test]
fn a_root_count_of_0_counts_the_tree_by_its_nodes_counts() {
    // PDFium doesn't count by a /Count of 0: it counts the tree, trusting
    // each node's own /Count as it would the root's, 1 and 1, though the
    // nodes list 2 and 1. The table ends at that count, 2: the first node's
    // pages, and 7 isn't reached.
    let objects = [
        catalog(),
        object(2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 0 >>"),
        object(
            3,
            "<< /Type /Pages /Parent 2 0 R /Kids [5 0 R 6 0 R] /Count 1 >>",
        ),
        object(4, "<< /Type /Pages /Parent 2 0 R /Kids [7 0 R] /Count 1 >>"),
        object(5, "<< /Type /Page /Parent 3 0 R >>"),
        object(6, "<< /Type /Page /Parent 3 0 R >>"),
        object(7, "<< /Type /Page /Parent 4 0 R >>"),
    ];
    let all = [5, 6].map(|n| (ObjectId::new(n, 0), 0));
    assert_eq!(ids(&open(&objects).unwrap()), all);
}

#[test]
fn a_page_tree_written_in_place_or_behind_two_references_is_walked() {
    let pages = |root: &str| {
        [
            object(1, &format!("<< /Type /Catalog /Pages {root} >>")),
            object(2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>"),
            object(3, "<< /Type /Page /MediaBox [0 0 100 200] >>"),
            object(4, "<< /Type /Page >>"),
            object(5, "2 0 R"),
        ]
    };
    let in_place = "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>";
    for root in [in_place, "5 0 R"] {
        let src = open(&pages(root)).unwrap();
        let all = [3, 4].map(|n| (ObjectId::new(n, 0), 0));
        assert_eq!(ids(&src), all, "{root}");
        assert_eq!(src.page_size(0), size(100.0, 200.0), "{root}");
    }
}

/// A file of one page, object 3, whose `/MediaBox` is object 5, `media`.
fn media_behind(media: &str) -> [(u32, Vec<u8>); 4] {
    [
        catalog(),
        object(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        object(3, "<< /Type /Page /Parent 2 0 R /MediaBox 5 0 R >>"),
        object(5, media),
    ]
}

#[test]
fn a_media_box_behind_two_references_is_letter() {
    // PDFium follows one reference to a page's box: what it finds there
    // is another, which is no box.
    let mut objects = media_behind("6 0 R").to_vec();
    objects.push(object(6, "[0 0 100 200]"));
    assert_eq!(open(&objects).unwrap().page_size(0), size(612.0, 792.0));
    let objects = media_behind("[0 0 100 200]");
    assert_eq!(open(&objects).unwrap().page_size(0), size(100.0, 200.0));
}

#[test]
fn a_media_box_naming_itself_opens_as_letter() {
    // One reference followed, a loop of them is never met.
    let src = open(&media_behind("5 0 R")).unwrap();
    assert_eq!(src.page_size(0), size(612.0, 792.0));
}

#[test]
fn inherited_resources_are_one_arc_across_pages() {
    // Pages 3 and 6 inherit the root's /Resources, 6 through node 4; page 5
    // has its own.
    let objects = [
        catalog(),
        object(
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 \
             /Resources << /Font << /F1 9 0 R >> >> >>",
        ),
        object(3, "<< /Type /Page /Parent 2 0 R >>"),
        object(4, "<< /Type /Pages /Parent 2 0 R /Kids [6 0 R] >>"),
        object(5, "<< /Type /Page /Parent 2 0 R /Resources << >> >>"),
        object(6, "<< /Type /Page /Parent 4 0 R >>"),
    ];
    let src = open(&objects).unwrap();
    let all = [3, 6, 5].map(|n| (ObjectId::new(n, 0), 0));
    assert_eq!(ids(&src), all);
    let resources = |i| src.page(i).unwrap().resources.clone().unwrap();
    assert!(matches!(resources(0).value(), Value::Dict(d) if d.has(b"Font")));
    assert!(Arc::ptr_eq(&resources(0), &resources(1)));
    assert!(!Arc::ptr_eq(&resources(0), &resources(2)));
}

#[test]
fn a_cap_broken_reading_a_kid_is_the_reader_s_own_error() {
    // The kid nests arrays past what the reader parses: the open fails
    // with the reader's error, not a page tree's.
    let nested = format!("{}{}", "[".repeat(300), "]".repeat(300));
    let objects = [
        catalog(),
        object(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        object(3, &format!("<< /Type /Page /Parent 2 0 R /A {nested} >>")),
    ];
    let result = open(&objects);
    assert!(
        matches!(result, Err(OpenError::Pdf(crate::pdf::Error::Limit(_)))),
        "{:?}",
        result.map(|src| src.page_count())
    );
}

/// The root's `/Kids`, `n` pages from object `first` on, under a root
/// with `keys`.
fn root(first: u32, n: u32, keys: &str) -> (u32, Vec<u8>) {
    let kids: String = (first..first + n).map(|k| format!("{k} 0 R ")).collect();
    object(2, &format!("<< /Type /Pages /Kids [{kids}] {keys} >>"))
}

#[test]
fn pages_share_what_they_hold_however_often_it_s_listed() {
    // 1,000 pages naming one /Annots array of 10,000 entries hold it once.
    let mut objects = vec![
        catalog(),
        root(4, 1000, ""),
        object(3, &format!("[{}]", "10 0 R ".repeat(10_000))),
    ];
    for n in 4..1004 {
        objects.push(object(n, "<< /Type /Page /Parent 2 0 R /Annots 3 0 R >>"));
    }
    let started = Instant::now();
    let src = open(&objects).unwrap();
    assert!(started.elapsed() < QUICK, "{:?}", started.elapsed());
    let annots = &src.page(0).unwrap().annots;
    assert_eq!((src.page_count(), annots.len()), (1000, 10_000));
    assert!(src.pages().iter().all(|p| Arc::ptr_eq(&p.annots, annots)));

    // The root lists a node 300 times, and the node its page 300 times:
    // 90,000 pages, whose own /Annots, /Contents and /Resources, written
    // in place, are held once.
    let objects = [
        catalog(),
        object(
            2,
            &format!("<< /Type /Pages /Kids [{}] >>", "3 0 R ".repeat(300)),
        ),
        object(
            3,
            &format!(
                "<< /Type /Pages /Parent 2 0 R /Kids [{}] >>",
                "4 0 R ".repeat(300)
            ),
        ),
        object(
            4,
            &format!(
                "<< /Type /Page /Parent 3 0 R /Annots [{}] /Contents [5 0 R 6 0 R] \
                 /Resources << /Font << /F1 7 0 R >> >> >>",
                "10 0 R ".repeat(100)
            ),
        ),
    ];
    let started = Instant::now();
    let src = open(&objects).unwrap();
    assert!(started.elapsed() < QUICK, "{:?}", started.elapsed());
    assert_eq!(src.page_count(), 90_000);
    let first = src.page(0).unwrap();
    for (i, page) in src.pages().iter().enumerate() {
        assert_eq!((page.id, page.repeat), (ObjectId::new(4, 0), i as u32));
        assert!(Arc::ptr_eq(&page.annots, &first.annots));
        assert!(Arc::ptr_eq(&page.contents, &first.contents));
        let resources = (page.resources.as_ref(), first.resources.as_ref());
        assert!(Arc::ptr_eq(resources.0.unwrap(), resources.1.unwrap()));
    }
}

#[test]
fn a_broken_object_every_page_names_is_read_once() {
    // 1,000 pages whose /MediaBox is object 3, an array of 300,000
    // numbers that never closes: parsed once, not once a page.
    let mut objects = vec![
        catalog(),
        root(4, 1000, ""),
        object(3, &format!("[{}", "1 ".repeat(300_000))),
    ];
    for n in 4..1004 {
        objects.push(object(n, "<< /Type /Page /Parent 2 0 R /MediaBox 3 0 R >>"));
    }
    let started = Instant::now();
    let src = open(&objects).unwrap();
    assert!(started.elapsed() < QUICK, "{:?}", started.elapsed());
    assert_eq!(src.page_count(), 1000);
    assert_eq!(src.page_size(999), size(612.0, 792.0));
    // And it fails as it failed the first time.
    for _ in 0..2 {
        assert!(matches!(
            src.pdf().get(ObjectId::new(3, 0)),
            Err(crate::pdf::Error::IndirectObject { .. })
        ));
    }
}

#[test]
fn the_root_s_count_ends_the_table_as_it_ends_pdfium_s() {
    let tree = |count: &str| {
        let mut objects = vec![catalog(), root(3, 3, &format!("/Count {count}"))];
        for n in 3..6 {
            objects.push(object(n, "<< /Type /Page /Parent 2 0 R >>"));
        }
        objects.push(object(9, "1"));
        objects
    };
    // PDFium loads no page past a /Count it counts by, a real's whole part
    // and a reference followed.
    for (count, pages) in [("2", 2), ("2.9", 2), ("9 0 R", 1)] {
        assert_eq!(open(&tree(count)).unwrap().page_count(), pages, "{count}");
    }
    // Past the tree, or one PDFium doesn't count by, the tree counts.
    for count in ["5", "0", "-1", "1048575", "(2)"] {
        assert_eq!(open(&tree(count)).unwrap().page_count(), 3, "{count}");
    }
    // The walk stops there, so a cycle past it isn't met, as PDFium
    // doesn't meet it.
    let objects = [
        catalog(),
        object(2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 1 >>"),
        object(3, "<< /Type /Page /Parent 2 0 R >>"),
        object(4, "<< /Type /Pages /Parent 2 0 R /Kids [2 0 R] >>"),
    ];
    assert_eq!(ids(&open(&objects).unwrap()), [(ObjectId::new(3, 0), 0)]);
}

#[test]
fn inherited_boxes_and_rotation_read_as_pdfium_reads_them() {
    let objects = [
        catalog(),
        object(
            2,
            "<< /Type /Pages /Kids [3 0 R 10 0 R 11 0 R 12 0 R 13 0 R 14 0 R 15 0 R] \
             /MediaBox 20 0 R /Rotate 135 >>",
        ),
        object(
            3,
            "<< /Type /Pages /Parent 2 0 R /Kids [4 0 R 5 0 R] /CropBox [50 50 550 750] >>",
        ),
        // Everything inherited: a crop of 500 × 700, a quarter turn.
        object(4, "<< /Type /Page /Parent 3 0 R >>"),
        // -90 is 270; its own media box, corners reversed, still clips the
        // crop it inherits.
        object(
            5,
            "<< /Type /Page /Parent 3 0 R /Rotate -90 /MediaBox [300 400 0 0] >>",
        ),
        // 45 is no turn; an empty media box is Letter, and the crop is
        // clipped to it.
        object(
            10,
            "<< /Type /Page /Parent 2 0 R /Rotate 45 /MediaBox [0 0 0 0] /CropBox [-100 -100 300 21 0 R] >>",
        ),
        // A crop outside the media box leaves nothing shown, as in PDFium.
        object(
            11,
            "<< /Type /Page /Parent 2 0 R /Rotate 0 /CropBox [700 0 900 100] >>",
        ),
        // UserUnit is kept, not applied; the other boxes are the page's own.
        object(
            12,
            "<< /Type /Page /Parent 2 0 R /Rotate 0 /UserUnit 2.5 /MediaBox [0 0 100 200] \
             /TrimBox [90 190 10 10] /BleedBox [0 0 100 200] /ArtBox [1 2 3] >>",
        ),
        // Inherited by /Parent, not by the path that listed it: from 3.
        object(13, "<< /Type /Page /Parent 3 0 R /Rotate 0 >>"),
        // A media box that isn't four numbers is Letter; a /Rotate that's
        // a real turns by its whole part.
        object(
            14,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612] /Rotate 180.9 >>",
        ),
        // A null /CropBox is found, as in PDFium, and isn't a box: 3's
        // crop isn't inherited past it, and the crop is the media box.
        object(15, "<< /Type /Page /Parent 3 0 R /CropBox null >>"),
        object(20, "[0 0 600 800]"),
        object(21, "300"),
    ];
    let src = open(&objects).unwrap();
    let geom = |i| src.page(i).unwrap().geom;
    let shown = |i| src.page_size(i);
    assert_eq!(src.page_count(), 8);

    assert_eq!(geom(0).media, Rect::new(0.0, 0.0, 600.0, 800.0));
    assert_eq!(geom(0).crop, Rect::new(50.0, 50.0, 550.0, 750.0));
    assert_eq!(geom(0).rotate, Rotation::R90);
    assert_eq!(shown(0), size(700.0, 500.0));

    assert_eq!(geom(1).rotate, Rotation::R270);
    assert_eq!(geom(1).visible(), Rect::new(50.0, 50.0, 300.0, 400.0));
    assert_eq!(shown(1), size(350.0, 250.0));

    assert_eq!(geom(2).rotate, Rotation::R0);
    assert_eq!(geom(2).media, Rect::LETTER);
    assert_eq!(geom(2).crop, Rect::new(0.0, 0.0, 300.0, 300.0));
    assert_eq!(shown(2), size(300.0, 300.0));

    assert_eq!(geom(3).crop, Rect::default());
    assert_eq!(shown(3), size(0.0, 0.0));

    let g = geom(4);
    assert_eq!((g.user_unit, shown(4)), (2.5, size(100.0, 200.0)));
    assert_eq!(g.trim, Some(Rect::new(10.0, 10.0, 90.0, 190.0)));
    assert_eq!(g.bleed, Some(Rect::new(0.0, 0.0, 100.0, 200.0)));
    assert_eq!(g.art, None);
    assert_eq!(geom(0).user_unit, 1.0);
    assert_eq!(geom(0).trim, None);

    assert_eq!(geom(5).crop, Rect::new(50.0, 50.0, 550.0, 750.0));
    assert_eq!(shown(5), size(500.0, 700.0));

    assert_eq!(
        (geom(6).media, geom(6).rotate),
        (Rect::LETTER, Rotation::R180)
    );
    assert_eq!(geom(7).crop, Rect::new(0.0, 0.0, 600.0, 800.0));
    assert_eq!(geom(7).rotate, Rotation::R90);
}

#[test]
fn per_page_annots_are_listed() {
    let objects = [
        catalog(),
        object(
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Resources 9 0 R >>",
        ),
        object(
            3,
            "<< /Type /Page /Parent 2 0 R /Annots [10 0 R 11 0 R] /Contents 20 0 R /StructParents 7 >>",
        ),
        object(
            4,
            "<< /Type /Page /Parent 2 0 R /Annots 12 0 R /Contents [21 0 R 22 0 R] \
             /Resources << /Font << >> >> >>",
        ),
        object(5, "<< /Type /Page /Parent 2 0 R /Annots 99 0 R >>"),
        object(9, "<< /Font << >> >>"),
        object(12, "[13 0 R << /Subtype /Link >>]"),
    ];
    let src = open(&objects).unwrap();
    let page = |i| src.page(i).unwrap();
    let annot_refs: Vec<_> = page(0).annots.iter().map(Object::as_reference).collect();
    assert_eq!(
        annot_refs,
        [Some(ObjectId::new(10, 0)), Some(ObjectId::new(11, 0))]
    );
    // An indirect /Annots is followed; a direct entry is kept as written.
    assert_eq!(page(1).annots.len(), 2);
    assert_eq!(page(1).annots[0].as_reference(), Some(ObjectId::new(13, 0)));
    assert!(matches!(page(1).annots[1].value(), Value::Dict(_)));
    // One that names nothing lists none.
    assert!(page(2).annots.is_empty());

    assert_eq!(&*page(0).contents, [ObjectId::new(20, 0)]);
    assert_eq!(
        &*page(1).contents,
        [ObjectId::new(21, 0), ObjectId::new(22, 0)]
    );
    assert!(page(2).contents.is_empty());
    assert_eq!(page(0).struct_parents, Some(7));
    assert_eq!(page(1).struct_parents, None);
    assert_eq!(
        page(0).resources.as_deref().and_then(Object::as_reference),
        Some(ObjectId::new(9, 0))
    );
    assert!(matches!(
        page(1).resources.as_ref().unwrap().value(),
        Value::Dict(_)
    ));
    assert_eq!(page(0).parents().collect::<Vec<_>>(), [ObjectId::new(2, 0)]);
    // Pages under one node share its chain.
    assert!(std::sync::Arc::ptr_eq(
        page(0).parent.as_ref().unwrap(),
        page(2).parent.as_ref().unwrap()
    ));
}

/// A file whose catalog has XMP `xml` as its `/Metadata`, deflated when
/// `deflate`.
fn with_xmp(xml: impl AsRef<[u8]>, deflate_xmp: bool) -> Result<Src, OpenError> {
    let xml = xml.as_ref();
    let (filter, data) = if deflate_xmp {
        ("/Filter /FlateDecode ", deflate(xml))
    } else {
        ("", xml.to_vec())
    };
    let metadata = stream(
        &format!(
            "/Type /Metadata /Subtype /XML {filter}/Length {}",
            data.len()
        ),
        &data,
    );
    open(&[
        object(1, "<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R >>"),
        object(2, "<< /Type /Pages /Kids [3 0 R] >>"),
        object(3, "<< /Type /Page /Parent 2 0 R >>"),
        (4, metadata),
    ])
}

/// XMP metadata holding `description`, its `rdf:Description`s.
pub(crate) fn xmp(description: &str) -> String {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
         {description}</rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
    )
}

/// The claims `pdfa` and `pdfua`, as [`Source::conformance`] gives them.
///
/// [`Source::conformance`]: super::Source::conformance
fn claimed(pdfa: Option<PdfA>, pdfua: Option<PdfUa>) -> Conformance {
    let mut claims = Conformance::default();
    (claims.pdfa, claims.pdfua) = (pdfa, pdfua);
    claims
}

/// A file whose packet has `description` as its `rdf:Description`s, read
/// for its claims.
fn claims(description: &str, deflate: bool) -> anyhow::Result<Conformance> {
    with_xmp(xmp(description), deflate).unwrap().conformance()
}

#[test]
fn conformance_reads_pdfa_and_pdfua_claims() {
    // Attributes.
    let attributes = "<rdf:Description rdf:about=\"\" \
         xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\" pdfaid:part=\"2\" pdfaid:conformance=\"b\"/>";
    let pdfa_2b = claimed(Some(PdfA::new(2, Some('B'))), None);
    assert_eq!(claims(attributes, false).unwrap(), pdfa_2b);
    assert_eq!(claims(attributes, true).unwrap(), pdfa_2b);
    assert_eq!(pdfa_2b.pdfa.unwrap().level, Some('B'));
    assert_eq!(pdfa_2b.pdfa.unwrap().to_string(), "PDF/A-2b");
    // Elements, under any prefix, and PDF/UA beside PDF/A.
    let elements = "<rdf:Description rdf:about=\"\" xmlns:a=\"http://www.aiim.org/pdfa/ns/id/\">\
         <a:part>3</a:part><a:conformance>A</a:conformance></rdf:Description>\
         <rdf:Description rdf:about=\"\" xmlns:pdfuaid=\"http://www.aiim.org/pdfua/ns/id/\">\
         <pdfuaid:part> 1 </pdfuaid:part></rdf:Description>";
    assert_eq!(
        claims(elements, false).unwrap(),
        claimed(Some(PdfA::new(3, Some('A'))), Some(PdfUa::new(1)))
    );
    // PDF/A-4 gives no level; a part in another namespace is no claim.
    let pdfa_4 = "<rdf:Description xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\" \
         xmlns:o=\"http://example.com/other/\" pdfaid:part=\"4\" o:part=\"9\"><o:part>8</o:part>\
         </rdf:Description>";
    assert_eq!(
        claims(pdfa_4, false).unwrap(),
        claimed(Some(PdfA::new(4, None)), None)
    );
    let named = PdfA::new(4, None);
    assert_eq!(named.to_string(), "PDF/A-4");
    assert_eq!(claims("", false).unwrap(), Conformance::default());
    let unreadable =
        "<rdf:Description xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\" pdfaid:part=\"two\"/>";
    assert!(claims(unreadable, false).is_err());
    // No /Metadata, no claims.
    let src = open(&[
        catalog(),
        object(2, "<< /Type /Pages /Kids [3 0 R] >>"),
        object(3, "<< /Type /Page /Parent 2 0 R >>"),
    ])
    .unwrap();
    assert_eq!(src.conformance().unwrap(), Conformance::default());
}

/// An attribute that doesn't read, or a claim's value that doesn't, is read
/// past: a claim elsewhere in the packet counts, and a value after one that
/// doesn't read is the claim's.
#[test]
fn a_claim_counts_past_what_doesn_t_read_elsewhere_in_the_packet() {
    // Unquoted, and given twice, before the claims and among them.
    let attributes = "<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
         dc:format=application/pdf/>\
         <rdf:Description rdf:about=\"\" rdf:about=\"\" \
         xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\" pdfaid:part=\"2\" pdfaid:conformance=\"B\"/>\
         <rdf:Description rdf:about=\"\" xmlns:pdfuaid=\"http://www.aiim.org/pdfua/ns/id/\">\
         <pdfuaid:part>1</pdfuaid:part></rdf:Description>";
    let both = claimed(Some(PdfA::new(2, Some('B'))), Some(PdfUa::new(1)));
    assert_eq!(claims(attributes, false).unwrap(), both);
    assert_eq!(claims(attributes, true).unwrap(), both);
    // A part with an entity no DOCTYPE declares, then one that reads.
    let value = "<rdf:Description rdf:about=\"\" xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\">\
         <pdfaid:part>&two;</pdfaid:part><pdfaid:part>3</pdfaid:part>\
         <pdfaid:conformance>u</pdfaid:conformance></rdf:Description>";
    assert_eq!(
        claims(value, false).unwrap(),
        claimed(Some(PdfA::new(3, Some('U'))), None)
    );
}

/// A catalog `/Metadata` that names a dictionary, or nothing, has no
/// packet: no claims, and no error.
#[test]
fn metadata_that_s_no_stream_claims_nothing() {
    for metadata in [
        "<< /Type /Metadata /Subtype /XML >>",
        "(pdfaid:part=\"2\")",
        "null",
    ] {
        let src = open(&[
            object(1, "<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R >>"),
            object(2, "<< /Type /Pages /Kids [3 0 R] >>"),
            object(3, "<< /Type /Page /Parent 2 0 R >>"),
            object(4, metadata),
        ])
        .unwrap();
        assert_eq!(
            src.conformance().unwrap(),
            Conformance::default(),
            "{metadata}"
        );
    }
    // One written in the catalog, and a reference to an object that isn't
    // there.
    for metadata in ["<< /Type /Metadata /Subtype /XML >>", "9 0 R"] {
        let src = open(&[
            object(
                1,
                &format!("<< /Type /Catalog /Pages 2 0 R /Metadata {metadata} >>"),
            ),
            object(2, "<< /Type /Pages /Kids [3 0 R] >>"),
            object(3, "<< /Type /Page /Parent 2 0 R >>"),
        ])
        .unwrap();
        assert_eq!(
            src.conformance().unwrap(),
            Conformance::default(),
            "{metadata}"
        );
    }
}

/// A packet in UTF-16 or UTF-32, which XMP allows and the reader reads
/// none of, can't be read: it isn't taken for one that claims nothing.
#[test]
fn a_packet_that_isn_t_utf8_can_t_be_read() {
    let packet = xmp("<rdf:Description rdf:about=\"\" \
         xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\" pdfaid:part=\"1\" pdfaid:conformance=\"B\"/>");
    let utf16 = |be: bool, mark: bool| -> Vec<u8> {
        let units = (mark.then_some(0xFEFF).into_iter()).chain(packet.encode_utf16());
        units
            .flat_map(|u| if be { u.to_be_bytes() } else { u.to_le_bytes() })
            .collect()
    };
    let utf32le: Vec<u8> = (packet.chars())
        .flat_map(|c| u32::from(c).to_le_bytes())
        .collect();
    let encoded = [
        utf16(true, true),
        utf16(true, false),
        utf16(false, true),
        utf16(false, false),
        utf32le,
    ];
    for (i, xml) in encoded.into_iter().enumerate() {
        let error = with_xmp(xml, false).unwrap().conformance().unwrap_err();
        assert!(
            format!("{error:#}").contains("isn't UTF-8"),
            "{i}: {error:#}"
        );
    }
    // The same packet in UTF-8, after its mark or not, claims PDF/A-1b.
    let pdfa_1b = claimed(Some(PdfA::new(1, Some('B'))), None);
    assert_eq!(
        with_xmp(&packet, false).unwrap().conformance().unwrap(),
        pdfa_1b
    );
    let marked = [&[0xEF, 0xBB, 0xBF][..], packet.as_bytes()].concat();
    assert_eq!(
        with_xmp(marked, false).unwrap().conformance().unwrap(),
        pdfa_1b
    );
}

/// XML that breaks off after a claim is an error, though the claim before
/// it was read: none is taken from a packet that doesn't read to its end.
#[test]
fn xml_that_breaks_off_after_a_claim_is_an_error() {
    let claim = "<rdf:Description rdf:about=\"\" \
         xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\" pdfaid:part=\"2\" pdfaid:conformance=\"B\"/>";
    // A tag left open where the packet ends, and an end tag that closes
    // another element.
    let whole = xmp(claim);
    let open = &whole[..whole.find("</rdf:RDF>").unwrap()];
    let broken = [
        format!("{open}<rdf:Description rdf:about="),
        xmp(&format!("{claim}</rdf:Description>")),
    ];
    for xml in broken {
        let error = with_xmp(xml, false).unwrap().conformance().unwrap_err();
        assert!(format!("{error:#}").contains("XMP metadata"), "{error:#}");
    }
}

#[test]
fn an_encrypted_source_keeps_how_it_is_encrypted() {
    let plain = plaintext();
    let mut states = rc4_aes_128_and_aes_256(&plain).to_vec();
    states.push(("attachment only", attachment_only(&plain)));
    for (name, state) in states {
        let expected_dict = Object::from(encrypt_dict_for(&plain, &state));
        for bytes in encrypted(&plain, &state, |_| {}) {
            let kind = BlobKind::file(Path::new("t.pdf"), Some(USER));
            let src = Store::new().open(bytes.into(), kind).unwrap();
            let crypt = src.crypt().unwrap();
            // Its /Encrypt as written, not decrypted, where the trailer
            // says; its /ID; and its filters.
            let at = src.pdf().trailer().get(b"Encrypt").unwrap();
            assert_eq!(crypt.at(), at.as_reference().unwrap(), "{name}");
            assert!(
                Object::from(crypt.dict().clone()).eq_value(&expected_dict),
                "{name}"
            );
            let [first, _] = src.xref().id.as_ref().unwrap();
            assert_eq!(crypt.first_id(), Some(&first[..]), "{name}");
            assert!(crypt.packs(), "{name}");
        }
    }
    let src = open(&[
        catalog(),
        object(2, "<< /Type /Pages /Kids [3 0 R] >>"),
        object(3, "<< /Type /Page /Parent 2 0 R >>"),
    ])
    .unwrap();
    assert!(src.crypt().is_none());
}

/// A source reads a field's font once a name, and keeps at most
/// [`super::FIELD_FONTS`] names: past them a font is read each time.
#[test]
fn a_source_keeps_at_most_its_cap_of_field_fonts() {
    let src = crate::forms::fixture::Form::new(1).source();
    let first = src.field_font(b"F0").unwrap();
    for i in 1..300 {
        src.field_font(format!("F{i}").as_bytes()).unwrap();
    }
    let kept = src.field_fonts.lock().unwrap().len();
    assert_eq!(kept, super::FIELD_FONTS);
    assert!(Arc::ptr_eq(&first, &src.field_font(b"F0").unwrap()));
    let past = src.field_font(b"F299").unwrap();
    let again = src.field_font(b"F299").unwrap();
    assert!(!Arc::ptr_eq(&past, &again), "read again, not kept");
}

/// A field's font that can't be read (here a reference chain past the
/// reader's depth) is kept as that failure, so it isn't read again and
/// its widgets are refused alike.
#[test]
fn a_field_font_that_fails_is_kept_as_its_failure() {
    use crate::pdf::forms::Error;
    let mut f = crate::forms::fixture::Form::new(1);
    let mut at = f.pdf.add_object(crate::pdf::dictionary! {
        "Type" => Object::name("Font"), "Subtype" => Object::name("Type1"),
        "BaseFont" => Object::name("Helvetica"),
    });
    for _ in 0..Limits::DEFAULT.reader.max_depth + 4 {
        at = f.pdf.add_object(Object::from(at));
    }
    let acroform = f.acroform;
    let fonts = (f.dict(acroform).get_mut(b"DR"))
        .and_then(|dr| dr.as_dict_mut())
        .and_then(|dr| dr.get_mut(b"Font"))
        .and_then(|fonts| fonts.as_dict_mut())
        .expect("the fixture's /DR /Font");
    fonts.set("Deep", at);
    let src = f.source();
    let capped = |read: &super::FieldFontRead| matches!(read, Err(Error::Unreadable(u)) if u.why == crate::pdf::Why::Capped);
    let read = src.field_font(b"Deep");
    assert!(capped(&read), "{read:?}");
    let kept = src.field_fonts.lock().unwrap().get(&b"Deep"[..]).cloned();
    assert!(kept.as_ref().is_some_and(capped), "{kept:?}");
    assert!(capped(&src.field_font(b"Deep")));
    // /DR's other fonts read as ever.
    assert!(!src.field_font(b"Helv").unwrap().is_fallback());
}

/// What a source keeps of a field font's read: the font, and each failure
/// another read would meet again; never the source's own I/O failing,
/// which the next read may not.
#[test]
fn a_field_font_s_read_is_kept_unless_the_source_s_io_failed() {
    use crate::pdf::forms::{Cap, Error};
    use crate::pdf::{Unreadable, Why};
    let font = crate::pdf::forms::FieldFont::standard(crate::pdf::fonts::Std14::Helvetica);
    assert!(super::keeps(&Ok(Arc::new(font))));
    assert!(super::keeps(&Err(Error::Limit(Cap::FontBudget))));
    let id = ObjectId::new(7, 0);
    for why in [
        Why::Damaged,
        Why::Limit,
        Why::Capped,
        Why::Locked,
        Why::Filter,
        Why::Detached,
    ] {
        let read = Err(Error::Unreadable(Unreadable::new(id, why, "no")));
        assert!(super::keeps(&read), "{why:?}");
    }
    let io = Err(Error::Unreadable(Unreadable::new(id, Why::Source, "no")));
    assert!(!super::keeps(&io));
}
