//! `sheaf-cli scan --ocr` end to end, through the binary: three pages of
//! printed text, rendered to images at 300 dpi, scanned back with their
//! text recognized. The words are found on their pages (every heading
//! word, nine in ten of the body's), each page draws one OCR block, and
//! every page's block is in the one glyphless font.
//!
//! Needs libpdfium and the OCR data (`scripts/fetch-pdfium.sh`,
//! `scripts/fetch-ocr-data.sh`), as CI fetches them: missing data fails
//! the scan with sheaf's own error, never a skip.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use common::{ok, run, scratch};
use lopdf::{Document as Pdf, Object, ObjectId, Stream, dictionary};
use sheaf_doc::content::Marker;

/// Each page's heading, the words OCR must find there, and its body: the
/// size it's set in and enough ordinary lines for orientation detection
/// to judge the page upright, nine in ten of whose words OCR must find.
const PAGES: [(&str, f32, &[&str]); 3] = [
    (
        "Invoice 4471 Paid",
        16.0,
        &[
            "Thank you for your business this quarter.",
            "The balance shown below was received in full",
            "by check on the fifth of the month, and no",
            "further amount is due on this account.",
            "Please keep this copy with your records for",
            "the year, and write to us at the address on",
            "the back if anything here looks wrong to you.",
            "Labor and materials are listed by the job,",
            "with the hours each crew spent on the site.",
            "Our office is open weekdays from eight to five.",
        ],
    ),
    (
        "Receipt Lumber Yard",
        14.0,
        &[
            "Twelve boards of pine, eight feet long, planed on",
            "all four sides, and two boxes of deck screws.",
            "One gallon of exterior stain in a walnut finish,",
            "a pair of work gloves and a roll of masking tape.",
            "Delivery to the house on Tuesday morning, with the",
            "driver to leave everything beside the garage door.",
            "Returns are taken within thirty days with this slip",
            "and the goods in the condition they were sold in.",
            "Ask at the counter about our seasonal discounts on",
            "fencing, garden beds and outdoor furniture kits.",
            "Thank you for shopping with us, and come again soon.",
        ],
    ),
    (
        "Mileage Log March",
        18.0,
        &[
            "Drove to the client office and back twice,",
            "then out to the warehouse for the count.",
            "Parking at the airport for the conference",
            "and a train ticket for the second trip.",
            "Each trip starts and ends at the office,",
            "with the odometer read before and after.",
            "Totals are carried to the expense report",
            "at the end of the month for approval.",
        ],
    ),
];

#[test]
fn scan_ocr_finds_the_words_in_one_block_a_page_and_one_font() {
    let dir = scratch("scan-ocr");
    let printed = letters(&dir);
    let images: Vec<String> = ok(&run(&[
        "export",
        "-q",
        printed.to_str().unwrap(),
        "-o",
        dir.join("images").to_str().unwrap(),
        "--format",
        "png",
        "--dpi",
        "300",
    ]))
    .lines()
    .map(String::from)
    .collect();
    assert_eq!(images.len(), 3);

    let out = dir.join("out.pdf");
    let mut args = vec!["scan", "-q", "--ocr", "--profile", "scanner", "-o"];
    args.push(out.to_str().unwrap());
    args.extend(images.iter().map(String::as_str));
    ok(&run(&args));
    let pdf = Pdf::load(&out).unwrap();
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    assert_eq!(pages.len(), 3);

    // The words, as a reader finds them: each page's heading on that page,
    // and nearly all of its body (Tesseract's builds differ a little).
    let words = dir.join("words.txt");
    ok(&run(&[
        "export",
        "-q",
        out.to_str().unwrap(),
        "--format",
        "text",
        "-o",
        words.to_str().unwrap(),
    ]));
    let text = std::fs::read_to_string(&words).unwrap();
    let tokens = |text: &str| -> BTreeSet<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    let lower = |words: &BTreeSet<String>| -> BTreeSet<String> {
        words.iter().map(|w| w.to_lowercase()).collect()
    };
    let found: Vec<BTreeSet<String>> = text.split('\u{c}').map(tokens).collect();
    assert_eq!(found.len(), 3, "{text}");
    for (n, ((heading, _, body), found)) in PAGES.iter().zip(&found).enumerate() {
        for word in tokens(heading) {
            assert!(
                found.contains(&word),
                "page {} has no {word:?}: {found:?}",
                n + 1
            );
        }
        let (body, found) = (lower(&tokens(&body.join(" "))), lower(found));
        let missing: Vec<&String> = body.iter().filter(|w| !found.contains(*w)).collect();
        assert!(
            missing.len() * 10 <= body.len(),
            "page {} lacks {} of its {} body words: {missing:?}",
            n + 1,
            missing.len(),
            body.len()
        );
    }

    // One OCR block a page, and one font for all of them.
    let mut fonts = BTreeSet::new();
    for (n, &page) in pages.iter().enumerate() {
        let ocr = contents(&pdf, page)
            .into_iter()
            .filter(|&id| {
                (pdf.get_object(id).ok())
                    .and_then(|o| Marker::of(&o.as_stream().ok()?.dict))
                    .is_some_and(|m| matches!(m, Marker::Ocr(_)))
            })
            .count();
        assert_eq!(ocr, 1, "page {} draws {ocr} OCR blocks", n + 1);
        let resources = pdf.get_dictionary(page).unwrap().get(b"Resources").unwrap();
        let font = (pdf.dereference(resources).unwrap().1.as_dict().unwrap())
            .get(b"Font")
            .unwrap();
        for (_, f) in pdf.dereference(font).unwrap().1.as_dict().unwrap() {
            fonts.insert(f.as_reference().unwrap());
        }
    }
    assert_eq!(fonts.len(), 1, "the pages draw in {fonts:?}");
    let font = pdf.get_dictionary(*fonts.first().unwrap()).unwrap();
    assert_eq!(
        font.get(b"BaseFont").and_then(Object::as_name).unwrap(),
        b"GlyphLessFont"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The ids of `page`'s content streams.
fn contents(pdf: &Pdf, page: ObjectId) -> Vec<ObjectId> {
    let contents = pdf.get_dictionary(page).unwrap().get(b"Contents").unwrap();
    match contents {
        Object::Reference(id) => match pdf.get_object(*id).unwrap() {
            Object::Array(items) => items.iter().map(|o| o.as_reference().unwrap()).collect(),
            _ => vec![*id],
        },
        Object::Array(items) => items.iter().map(|o| o.as_reference().unwrap()).collect(),
        other => panic!("/Contents is {other:?}"),
    }
}

/// `letters.pdf` in `dir`: [`PAGES`] on Letter pages in Helvetica, a 36 pt
/// heading over each body.
fn letters(dir: &Path) -> PathBuf {
    let mut pdf = Pdf::with_version("1.7");
    let pages_id = pdf.new_object_id();
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let kids: Vec<Object> = PAGES
        .iter()
        .map(|(heading, size, body)| {
            let mut content = format!("BT /F1 36 Tf 72 690 Td ({heading}) Tj ET\n");
            for (i, line) in body.iter().enumerate() {
                let y = 630.0 - i as f32 * size * 1.6;
                content += &format!("BT /F1 {size} Tf 72 {y} Td ({line}) Tj ET\n");
            }
            let content = pdf.add_object(Stream::new(dictionary! {}, content.into_bytes()));
            pdf.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id, "Contents" => content,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
            })
            .into()
        })
        .collect();
    pdf.objects.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 3 }.into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    pdf.trailer.set("Root", catalog);
    let path = dir.join("letters.pdf");
    pdf.save(&path).unwrap();
    path
}
