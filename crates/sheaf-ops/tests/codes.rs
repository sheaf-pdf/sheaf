//! The codes checks that need a real PDF page (`sheaf_image::codes`):
//! sheaf-image has no font, so its pages are glyph-like strokes, and real
//! glyphs come from PDFium here. A page of text drawn as separators are
//! found (proposal 15), its content alone, reads as nothing, and so does
//! every page of the corpus's scans (an ignored tier).

mod common;

use std::time::Instant;

use anyhow::{Context, Result};
use lopdf::{Document as Pdf, Object, Stream, dictionary};
use sheaf_doc::forms::corpus::{Corpus, Entry, each_entry};
use sheaf_image::codes::fixture::{Degradation::Threshold, degrade};
use sheaf_image::codes::{self, Code, PageCodes, Patch, Reader as _, Symbology};
use sheaf_render::Renderer;
use sheaf_render::pdfium::PdfiumEngine;

/// The words [`real_text_page`] draws, as a tax form's.
const WORDS: &str = "the of and to for your if or on line from enter amount total income \
    wages salaries tips taxable interest dividends qualified ordinary \
    IRA distributions pensions annuities Social Security benefits \
    capital gain loss attach Schedule required spouse adjusted gross \
    standard deduction itemized business credit child dependent \
    estimated payments refund owed Form W-2 1099-INT see instructions \
    2025 8a 1z 15,250 Part II check box here";

/// One Letter page of real text, as a form's or a letter's: a header in
/// Helvetica-Bold over a rule, 36 lines of words in Helvetica at 11 pt, a
/// 5 × 9 ruled table of amounts and a footer, its content written as
/// bytes. Both fonts are of the standard 14, which PDFium carries itself,
/// so the page draws the same on every system.
fn real_text_page() -> Vec<u8> {
    let mut rng = fastrand::Rng::with_seed(3);
    let words: Vec<&str> = WORDS.split(' ').collect();
    let mut content = String::from(
        "BT /F2 16 Tf 72 736 Td (Schedule 1  Additional Income and Adjustments) Tj ET\n\
         72 726 468 1.5 re f\n\
         BT /F1 11 Tf 12 TL 72 706 Td\n",
    );
    for _ in 0..36 {
        let mut line = String::new();
        while line.len() < 76 {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(words[rng.usize(..words.len())]);
        }
        content += &format!("({line}) Tj T*\n");
    }
    content += "ET\n";
    // The table: 9 rows of 20 pt and 5 columns of 93.6, 0.75 pt rules.
    let (left, top) = (72.0, 262.0);
    for r in 0..=9 {
        content += &format!("{left} {} 468 0.75 re f\n", top - 20.0 * f64::from(r));
    }
    for c in 0..=5 {
        content += &format!(
            "{:.1} {} 0.75 180 re f\n",
            left + 93.6 * f64::from(c),
            top - 180.0
        );
    }
    for r in 0..9 {
        for c in 0..5 {
            let cents = rng.u32(..10_000_000);
            let (x, y) = (
                left + 93.6 * f64::from(c) + 8.0,
                top - 20.0 * f64::from(r) - 14.0,
            );
            content += &format!(
                "BT /F1 10 Tf {x:.1} {y} Td ({}.{:02}) Tj ET\n",
                cents / 100,
                cents % 100
            );
        }
    }
    content += "BT /F1 8 Tf 72 50 Td (Keep a copy of this schedule for your records.) Tj ET\n";

    let mut pdf = Pdf::with_version("1.7");
    let font = |pdf: &mut Pdf, name: &str| {
        pdf.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => name,
            "Encoding" => "WinAnsiEncoding",
        })
    };
    let (regular, bold) = (
        font(&mut pdf, "Helvetica"),
        font(&mut pdf, "Helvetica-Bold"),
    );
    let pages = pdf.new_object_id();
    let contents = pdf.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let page = pdf.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Contents" => contents,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => regular, "F2" => bold } },
    });
    pdf.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn a_page_of_real_text_reads_as_nothing() {
    let engine = PdfiumEngine::open_bytes(real_text_page().into(), None).unwrap();
    let nothing = PageCodes {
        patch: None,
        code: None,
    };
    for dpi in [150, 200] {
        let gray = common::drawn(&engine, 0, dpi as f32).unwrap();
        // Drawn: a page whose text didn't draw would read nothing too.
        let ink = gray.pixels().filter(|p| p.0[0] < 128).count();
        assert!(
            ink * 50 > gray.len(),
            "{dpi} dpi: {ink} of {} pixels ink",
            gray.len()
        );
        let (bw, _) = degrade(&gray, dpi, &[Threshold(128)]);
        for (tone, image) in [("gray", gray), ("1-bit", bw)] {
            let found = codes::read(
                &image,
                dpi as f32,
                &Symbology::ALL,
                &|_| true,
                &codes::Rxing,
            );
            assert_eq!(found, nothing, "{dpi} dpi {tone}");
        }
    }
}

// ----- The corpus's scans, the negative set ---------------------------------

/// Corpus paths, comma-separated, to run [`scans_hold_no_separators`] on
/// alone (`SHEAF_CODES_ONLY=scans/safedocs/0000022.pdf,…`): for looking at
/// a few files by hand. Unset, it takes every file.
const ONLY: &str = "SHEAF_CODES_ONLY";

/// The corpus's scans, files and pages, as `scans/manifest.tsv` sums them.
const SCANS: (usize, usize) = (709, 5697);

/// What sheaf's sheet's text starts with: a literal here until slice 4
/// defines the sheet.
const SHEET: &str = "SHEAFSEP";

/// What the Code 39 text on Kodak's and NAPS2's patch sheets starts with
/// (`PATCH1` … `PATCHT`), which slice 5 reads as a patch.
const PATCH_TEXT: &str = "PATCH";

/// A sheaf sheet's code.
fn is_sheet(code: &Code) -> bool {
    code.symbology == Symbology::Qr && code.text.starts_with(SHEET)
}

/// A patch sheet's Code 39.
fn is_patch(code: &Code) -> bool {
    code.symbology == Symbology::Code39 && code.text.starts_with(PATCH_TEXT)
}

/// What a page of a scan carried, read as separators are found.
struct PageRead {
    /// Counted from 1, as a viewer numbers pages.
    page: u32,
    patches: Vec<Patch>,
    /// Read with every symbology, taking only a sheaf code or a patch
    /// sheet's Code 39, as `separate` reads: each symbology's decode runs
    /// on a page with neither, past any other code.
    separator: Option<Code>,
    /// Read with every symbology, taking the first code.
    first: Option<Code>,
}

/// Each page of `entry`'s file drawn at 150 dpi and read; an error when
/// the file doesn't open, a page doesn't draw, or PDFium's page count
/// isn't the manifest's.
fn scan_codes(corpus: &Corpus, entry: &Entry) -> Result<Vec<PageRead>> {
    let bytes = std::fs::read(corpus.path(entry))?;
    let engine = PdfiumEngine::open_bytes(bytes.into(), None)?;
    let count = engine.page_count();
    anyhow::ensure!(
        count == entry.pages,
        "{count} pages, the manifest's {}",
        entry.pages
    );
    (0..count)
        .map(|i| {
            let gray =
                common::drawn(&engine, i, 150.0).with_context(|| format!("page {}", i + 1))?;
            Ok(PageRead {
                page: i + 1,
                patches: codes::patches(&gray, 150.0),
                separator: codes::Rxing
                    .read(&gray, &Symbology::ALL, &|c| is_sheet(c) || is_patch(c)),
                first: codes::Rxing.read(&gray, &Symbology::ALL, &|_| true),
            })
        })
        .collect()
}

/// Proposal 22's negative set: every page of the corpus's scans drawn at
/// 150 dpi in gray, its content alone, and read as `separate` will read
/// it. It finds no sheaf code (a QR code whose text starts [`SHEET`]) and
/// no patch: bars, or a Code 39 whose text starts [`PATCH_TEXT`], each
/// looked for past every other code on the page. Any found is a false
/// positive to fix in the reader, never a file to leave out. The first
/// code a page gives, read for every symbology, is listed as the
/// baseline: they're content, like the forms' Code 39 ids. PDFium draws
/// one page at a time, under its lock, so this takes minutes.
#[test]
#[ignore = "corpus tier: cargo test -p sheaf-ops --test codes -- --ignored --nocapture"]
fn scans_hold_no_separators() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let entries = common::narrowed(ONLY, corpus.scans().unwrap(), |e| &e.path);
    let started = Instant::now();
    let outcomes = each_entry(&entries, |entry| scan_codes(&corpus, entry));
    let took = started.elapsed();
    let (mut files, mut pages) = (0, 0);
    let (mut sheets, mut patches, mut others, mut failed) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (entry, outcome) in entries.iter().zip(outcomes) {
        let read = match outcome {
            Ok(read) => read,
            Err(e) => {
                failed.push(format!("{}: {e:#}", entry.path));
                continue;
            }
        };
        files += 1;
        pages += read.len();
        for page in read {
            let at = format!("{} p.{}", entry.path, page.page);
            match page.separator {
                Some(code) if is_sheet(&code) => sheets.push(format!("{at}: {:?}", code.text)),
                Some(code) => patches.push(format!("{at}: Code 39 {:?}", code.text)),
                None => {}
            }
            for p in &page.patches {
                patches.push(format!(
                    "{at}: {:?} bars along the {:?} edge, {:.2} in in, {:.2} in long",
                    p.code, p.edge, p.inset, p.length
                ));
            }
            if let Some(code) = page.first.filter(|c| !is_sheet(c) && !is_patch(c)) {
                others.push(format!("{at} {:?}: {:?}", code.symbology, code.text));
            }
        }
    }
    others.sort();
    println!("{files} files, {pages} pages drawn at 150 dpi and read in {took:.0?}");
    println!("{} sheaf codes:", sheets.len());
    for found in &sheets {
        println!("  {found}");
    }
    println!("{} patches:", patches.len());
    for found in &patches {
        println!("  {found}");
    }
    println!("{} other codes, the first each page gives:", others.len());
    for found in &others {
        println!("  {found}");
    }
    for failure in &failed {
        println!("{failure}");
    }
    assert!(failed.is_empty(), "{} files failed", failed.len());
    if std::env::var_os(ONLY).is_none() {
        assert_eq!((files, pages), SCANS, "files and pages read");
    }
    assert!(sheets.is_empty(), "sheaf codes on scans: {sheets:#?}");
    assert!(patches.is_empty(), "patches on scans: {patches:#?}");
}
