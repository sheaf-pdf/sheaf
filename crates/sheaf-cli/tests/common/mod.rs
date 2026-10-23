//! What `sheaf edit`'s tests share: a scratch folder, the binary, and a
//! binder whose things each way of naming them finds: pages labelled
//! i, ii, 1 and 2; bookmarks "Part I" (over "Ch. 1") and "Part II";
//! another tool's note `n1` on page 1 and highlight `h1` on page 3; and
//! beside it, a one-page `appendix.pdf` and a `logo.png`.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use lopdf::{Dictionary, Document as Pdf, Object, ObjectId, Stream, dictionary};

/// A new, empty folder for test `name`.
pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sheaf-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn sheaf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sheaf-cli"))
}

/// `sheaf-cli args…`, run to its end.
pub fn run(args: &[&str]) -> Output {
    sheaf().args(args).output().unwrap()
}

/// Its standard output, once it succeeded.
pub fn ok(run: &Output) -> String {
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout.clone()).unwrap()
}

/// The binder, `in.pdf`, and `appendix.pdf` and `logo.png` beside it, in
/// `dir`.
pub fn binder(dir: &Path) -> PathBuf {
    let mut pdf = Pdf::with_version("1.7");
    let pages_id = pdf.new_object_id();
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let page_ids: Vec<ObjectId> = (1..=4)
        .map(|n| page(&mut pdf, pages_id, font, &format!("Page {n}")))
        .collect();
    let comment = |pdf: &mut Pdf, subtype: &str, who: &str, name: &str, text: &str| {
        pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => subtype,
            "Rect" => vec![72.into(), 600.into(), 200.into(), 620.into()],
            "Contents" => Object::string_literal(text),
            "T" => Object::string_literal(who),
            "NM" => Object::string_literal(name),
            "M" => Object::string_literal("D:20261001120000"),
        })
    };
    let note = comment(&mut pdf, "Text", "Ann", "n1", "Tie to the ledger, please");
    let mark = comment(&mut pdf, "Highlight", "Bo", "h1", "");
    for (page, annot) in [(page_ids[0], note), (page_ids[2], mark)] {
        pdf.get_dictionary_mut(page)
            .unwrap()
            .set("Annots", vec![annot.into()]);
    }
    let kids: Vec<Object> = page_ids.iter().map(|&id| id.into()).collect();
    pdf.objects.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 4 }.into(),
    );
    let outlines = pdf.new_object_id();
    let dest = |page: ObjectId| -> Object {
        vec![
            page.into(),
            "XYZ".into(),
            Object::Null,
            Object::Null,
            Object::Null,
        ]
        .into()
    };
    let (part1, ch1, part2) = (
        pdf.new_object_id(),
        pdf.new_object_id(),
        pdf.new_object_id(),
    );
    let item = |title: &str, parent: ObjectId, page: ObjectId| -> Dictionary {
        dictionary! {
            "Title" => Object::string_literal(title), "Parent" => parent, "Dest" => dest(page),
        }
    };
    let mut p1 = item("Part I", outlines, page_ids[0]);
    p1.set("First", ch1);
    p1.set("Last", ch1);
    p1.set("Count", 1);
    p1.set("Next", part2);
    let c1 = item("Ch. 1", part1, page_ids[1]);
    let mut p2 = item("Part II", outlines, page_ids[2]);
    p2.set("Prev", part1);
    pdf.objects.insert(part1, p1.into());
    pdf.objects.insert(ch1, c1.into());
    pdf.objects.insert(part2, p2.into());
    pdf.objects.insert(
        outlines,
        dictionary! { "Type" => "Outlines", "First" => part1, "Last" => part2, "Count" => 3 }
            .into(),
    );
    let labels = dictionary! {
        "Nums" => vec![
            0.into(), dictionary! { "S" => "r" }.into(),
            2.into(), dictionary! { "S" => "D" }.into(),
        ],
    };
    let catalog = pdf.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages_id, "Outlines" => outlines, "PageLabels" => labels,
    });
    pdf.trailer.set("Root", catalog);
    let path = dir.join("in.pdf");
    pdf.save(&path).unwrap();
    appendix(&dir.join("appendix.pdf"));
    image::RgbImage::from_pixel(60, 30, image::Rgb([30, 90, 200]))
        .save(dir.join("logo.png"))
        .unwrap();
    path
}

/// A one-page PDF saying "Appendix", at `path`.
fn appendix(path: &Path) {
    let mut pdf = Pdf::with_version("1.7");
    let pages_id = pdf.new_object_id();
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let only = page(&mut pdf, pages_id, font, "Appendix");
    pdf.objects.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => vec![only.into()], "Count" => 1 }.into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    pdf.trailer.set("Root", catalog);
    pdf.save(path).unwrap();
}

/// A Letter page of `pages_id`'s saying `text` in `font`.
fn page(pdf: &mut Pdf, pages_id: ObjectId, font: ObjectId, text: &str) -> ObjectId {
    let content = format!("BT /F1 24 Tf 72 700 Td ({text}) Tj ET");
    let content = pdf.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    pdf.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
    })
}

/// `edits`, an envelope's JSON, written to `name` in `dir`.
pub fn edits(dir: &Path, name: &str, edits: &serde_json::Value) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_string_pretty(edits).unwrap()).unwrap();
    path
}
