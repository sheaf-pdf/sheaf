//! Codes drawn clean on white pages: QR modules read back, and which
//! symbologies a read finds, in what order, past codes the caller turns
//! down. Then each symbology on a page
//! of text, scanned every way (`fixture::scanned_every_way`), read back,
//! and pages with no code read as nothing. Last, a page's patch code and
//! barcode read together ([`read`]).

use super::fixture::Degradation::*;
use super::fixture::*;
use super::*;
use rxing::{RXingResultMetadataType, RXingResultMetadataValue};

/// `modules` alone on white, `px` pixels a module, with a quiet zone of
/// four modules.
fn framed(modules: &Modules, px: u32) -> GrayImage {
    let mut img = white_page((modules.width() + 8) * px, (modules.height() + 8) * px);
    paste(&mut img, modules, 4 * px, 4 * px, px, px);
    img
}

/// What a QR code drawn from `text` reads as, through rxing directly.
fn decoded(text: &str) -> RXingResult {
    let img = framed(&qr(text).unwrap(), 4);
    let source = Luma8Source::new_with_slice(img.as_raw(), img.width(), img.height()).unwrap();
    let mut bitmap = BinaryBitmap::new(HybridBinarizer::new(source));
    let hints = DecodeHints {
        PossibleFormats: Some(HashSet::from([BarcodeFormat::QR_CODE])),
        ..Default::default()
    };
    MultiFormatReader::default()
        .decode_with_hints(&mut bitmap, &hints)
        .unwrap()
}

#[test]
fn a_qr_code_s_modules_read_back_as_their_text() {
    let modules = qr(SHEET).unwrap();
    assert_eq!(modules.width(), modules.height());
    for px in [4, 9] {
        let code = Rxing.read(&framed(&modules, px), &[Symbology::Qr], &|_| true);
        assert_eq!(
            code,
            Some(Code {
                symbology: Symbology::Qr,
                text: SHEET.into()
            }),
            "{px} px a module"
        );
    }
}

#[test]
fn titles_outside_ascii_read_back_as_utf_8() {
    // Latin-1 alone would be written as Latin-1 without the UTF-8 hint;
    // the rest falls back to UTF-8 on its own.
    for title in ["Müller & Söhne ½", "Müller & Söhne — 株式会社 ½ €"] {
        let text = format!("SHEAFSEP1|a=bookmark|l=1|p=keep|t={title}");
        let found = decoded(&text);
        assert_eq!(found.getText(), text);
        assert_eq!(found.getRawBytes(), text.as_bytes(), "{title}");
    }
}

#[test]
fn qr_codes_are_level_m() {
    let found = decoded(SHEET);
    let level = found
        .getRXingResultMetadata()
        .get(&RXingResultMetadataType::ERROR_CORRECTION_LEVEL);
    assert!(
        matches!(level, Some(RXingResultMetadataValue::ErrorCorrectionLevel(l)) if l == "M"),
        "{level:?}"
    );
}

#[test]
fn the_same_text_makes_the_same_modules() {
    assert_eq!(qr(SHEET).unwrap(), qr(SHEET).unwrap());
    // Pinned as well, so a change in rxing's encoder, which would change
    // every sheet's bytes, is seen.
    let modules = qr("SHEAFSEP1|t=").unwrap();
    let picture: Vec<String> = (0..modules.height())
        .map(|y| {
            (0..modules.width())
                .map(|x| if modules.dark(x, y) { '#' } else { '.' })
                .collect()
        })
        .collect();
    assert_eq!(
        picture,
        [
            "#######.###.#.#######",
            "#.....#.#.#...#.....#",
            "#.###.#...##..#.###.#",
            "#.###.#.###.#.#.###.#",
            "#.###.#..#.#..#.###.#",
            "#.....#.......#.....#",
            "#######.#.#.#.#######",
            "........#.##.........",
            "#.##.###.##...#..#.##",
            ".#.#...#...###..###..",
            "...#..##.#..#.#....#.",
            "#...##.#...##..##..#.",
            ".##..##.#.#.##...#...",
            "........##.#.##.#...#",
            "#######.#..#...##....",
            "#.....#.####...#....#",
            "#.###.#..#....##.#...",
            "#.###.#.#....#.###...",
            "#.###.#.##.#.###..#..",
            "#.....#...#.#.#....#.",
            "#######.##.#...###...",
        ]
    );
}

#[test]
fn the_longest_title_fits_version_9() {
    // 120 bytes of UTF-8: "Ü" is two.
    let title = "Ü".repeat(60);
    assert_eq!(title.len(), 120);
    let modules = qr(&format!("SHEAFSEP1|a=bookmark|l=2|p=keep|t={title}")).unwrap();
    assert_eq!(modules.width(), modules.height());
    // Version 9 is 53 modules: about 40 mm at 0.75 mm a module.
    assert!(modules.width() <= 53, "{} modules", modules.width());
}

#[test]
fn only_the_symbologies_asked_for_are_read() {
    let page = framed(&qr(SHEET).unwrap(), 6);
    assert_eq!(Rxing.read(&page, &[Symbology::Code128], &|_| true), None);
    assert_eq!(Rxing.read(&page, &[], &|_| true), None);
    assert_eq!(
        Rxing.read(&page, &Symbology::ALL, &|_| true),
        Some(Code {
            symbology: Symbology::Qr,
            text: SHEET.into()
        })
    );
}

#[test]
fn a_code_not_asked_for_never_hides_one_asked_for() {
    let mut page = white_page(1700, 1000);
    paste(
        &mut page,
        &encode(Symbology::DataMatrix, "DM-0001"),
        100,
        100,
        8,
        8,
    );
    let bars = encode(Symbology::Code128, "Schedule A");
    paste(&mut page, &bars, 100, 600, 4, 150);
    assert_eq!(
        Rxing.read(&page, &[Symbology::Code128], &|_| true),
        Some(Code {
            symbology: Symbology::Code128,
            text: "Schedule A".into()
        })
    );
}

/// A white page with each of `codes` (a symbology and its text) drawn on
/// it as [`SAMPLES`] draws that symbology, one below another. rxing's 1D
/// reader starts at a page's middle row, so it meets the last code
/// first.
fn page_of(codes: &[(Symbology, &str)]) -> GrayImage {
    let mut page = white_page(1700, 450 * codes.len() as u32 + 100);
    for (i, &(symbology, text)) in codes.iter().enumerate() {
        let &(_, _, module_px, row_px) = SAMPLES.iter().find(|s| s.0 == symbology).unwrap();
        let y = 100 + 450 * i as u32;
        paste(
            &mut page,
            &encode(symbology, text),
            150,
            y,
            module_px,
            row_px,
        );
    }
    page
}

#[test]
fn codes_asked_for_read_in_a_fixed_order() {
    // Each pair on one page, read for both (the later one named first and
    // drawn where a 1D read meets it first), gives the first of ALL's
    // order every time, and the other read alone.
    let pairs = [
        (Symbology::Qr, Symbology::DataMatrix),
        (Symbology::DataMatrix, Symbology::Pdf417),
        (Symbology::Pdf417, Symbology::Code39),
        (Symbology::Code39, Symbology::Code128),
    ];
    for (first, then) in pairs {
        let text = |s| SAMPLES.iter().find(|x| x.0 == s).unwrap().1;
        let page = page_of(&[(first, text(first)), (then, text(then))]);
        for _ in 0..20 {
            let code = Rxing.read(&page, &[then, first], &|_| true);
            assert_eq!(
                code.map(|c| c.symbology),
                Some(first),
                "{first:?}, {then:?}"
            );
        }
        assert_eq!(
            Rxing.read(&page, &[then], &|_| true),
            Some(Code {
                symbology: then,
                text: text(then).into()
            })
        );
    }
}

#[test]
fn a_code_turned_down_never_hides_a_later_one() {
    // A code counts only if its text matches (proposal 15): a vendor's QR
    // code before a rule's Code 128, a Data Matrix before a patch sheet's
    // Code 39, and a patent's Code 39 id before a rule's Code 128 (drawn
    // where a 1D read meets it first), each turned down.
    for (content, wanted, text) in [
        (
            (Symbology::Qr, "https://example.com/help"),
            Symbology::Code128,
            "Schedule A",
        ),
        (
            (Symbology::DataMatrix, "0205485000128"),
            Symbology::Code39,
            "PATCHT",
        ),
        (
            (Symbology::Code39, "US009782084B2"),
            Symbology::Code128,
            "Schedule A",
        ),
    ] {
        let page = page_of(&[(wanted, text), content]);
        let asked = [content.0, wanted];
        let first = Rxing.read(&page, &asked, &|_| true);
        assert_eq!(first.map(|c| c.symbology), Some(content.0));
        let code = Rxing.read(&page, &asked, &|c| c.text == text);
        assert_eq!(
            code,
            Some(Code {
                symbology: wanted,
                text: text.into()
            })
        );
        assert_eq!(Rxing.read(&page, &asked, &|_| false), None);
    }
}

#[test]
#[should_panic(expected = "module (21, 0) of 21 × 21")]
fn a_module_past_the_code_s_side_panics() {
    // Not the next row's first module.
    qr("SHEAFSEP1|t=").unwrap().dark(21, 0);
}

/// Reads `symbology`'s sample code page ([`SAMPLES`]) scanned every way,
/// with `symbology` alone and with every symbology, as its text: every
/// miss is listed, so a run shows them all.
fn reads_back_scanned_every_way(symbology: Symbology) {
    let &(_, text, module_px, row_px) = SAMPLES.iter().find(|s| s.0 == symbology).unwrap();
    let want = Some(Code {
        symbology,
        text: text.into(),
    });
    let mut misses = Vec::new();
    let scans = scanned_every_way(&code_page(symbology, text, module_px, row_px));
    for scan in &scans {
        for (asked, symbologies) in [("alone", &[symbology][..]), ("with all", &Symbology::ALL)] {
            let found = Rxing.read(&scan.image, symbologies, &|_| true);
            if found != want {
                misses.push(format!("{}, {asked}: {found:?}", scan.label));
            }
        }
    }
    assert!(
        misses.is_empty(),
        "{} of {} reads missed:\n{}",
        misses.len(),
        2 * scans.len(),
        misses.join("\n")
    );
}

#[test]
fn qr_reads_back_scanned_every_way() {
    reads_back_scanned_every_way(Symbology::Qr);
}

#[test]
fn data_matrix_reads_back_scanned_every_way() {
    reads_back_scanned_every_way(Symbology::DataMatrix);
}

#[test]
fn code_39_reads_back_scanned_every_way() {
    reads_back_scanned_every_way(Symbology::Code39);
}

#[test]
fn code_128_reads_back_scanned_every_way() {
    reads_back_scanned_every_way(Symbology::Code128);
}

#[test]
fn pdf417_reads_back_scanned_every_way() {
    reads_back_scanned_every_way(Symbology::Pdf417);
}

#[test]
fn blank_text_and_rules_pages_read_as_nothing() {
    let mut found = Vec::new();
    let mut reads = 0;
    for (name, page) in [
        ("blank", blank_page()),
        ("text", text_page(42, None)),
        ("rules", rules_page()),
    ] {
        for (skew, steps) in [
            ("as is", &[][..]),
            ("skewed +3°", &[Skew(3.0)]),
            ("skewed -3°", &[Skew(-3.0)]),
        ] {
            let (skewed, _) = degrade(&page, 300, steps);
            for dpi in [300, 200, 150] {
                let (gray, _) = degrade(&skewed, 300, &[Dpi(dpi)]);
                let (bw, _) = degrade(&gray, dpi, &[Threshold(128)]);
                for (tone, scan) in [("gray", gray), ("1-bit", bw)] {
                    reads += 1;
                    if let Some(code) = Rxing.read(&scan, &Symbology::ALL, &|_| true) {
                        found.push(format!("{name} page, {dpi} dpi {tone}, {skew}: {code:?}"));
                    }
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} of {reads} pages read a code:\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn every_degradation_changes_the_page() {
    // A degradation that did nothing would leave the matrix reading the
    // clean page. A piece of a page will do.
    let page = image::imageops::crop_imm(&text_page(7, None), 250, 180, 1000, 600).to_image();
    for (name, steps) in VARIATIONS.iter().skip(1).chain(BENCH.iter().skip(1)) {
        let (scan, dpi) = degrade(&page, 300, steps);
        assert_ne!(scan, page, "{name}");
        let want = [200, 150]
            .into_iter()
            .find(|d| name.contains(&d.to_string()));
        assert_eq!(dpi, want.unwrap_or(300), "{name}");
    }
    let (turned, _) = degrade(&page, 300, &[Turn(90)]);
    assert_eq!(turned.dimensions(), (600, 1000));
    let (scan, dpi) = degrade(&page, 300, &[Dpi(150), Threshold(128)]);
    assert_eq!((scan.dimensions(), dpi), ((500, 300), 150));
    assert!(scan.pixels().all(|p| p.0[0] == 0 || p.0[0] == 255));
}

/// `page`, at 300 dpi, as the reads below take it: at 300 and 150 dpi in
/// gray, and at 150 in black and white.
fn at_300_and_150(page: &GrayImage) -> [(&'static str, f32, GrayImage); 3] {
    let (gray, _) = degrade(page, 300, &[Dpi(150)]);
    let (bw, _) = degrade(&gray, 150, &[Threshold(128)]);
    [
        ("300 dpi gray", 300.0, page.clone()),
        ("150 dpi gray", 150.0, gray),
        ("150 dpi 1-bit", 150.0, bw),
    ]
}

#[test]
fn a_sheet_with_patch_bars_reads_both() {
    // Proposal 13's sheet may carry patch T bars beside its QR code.
    let page = code_page(Symbology::Qr, SHEET, 9, 9);
    let page = with_patch_bars(page, PatchCode::T, PatchLayout::KodakSheet);
    let want = PageCodes {
        patch: Some(PatchCode::T),
        code: Some(Code {
            symbology: Symbology::Qr,
            text: SHEET.into(),
        }),
    };
    for (scan, dpi, image) in at_300_and_150(&page) {
        assert_eq!(
            read(&image, dpi, &[Symbology::Qr], &|_| true, &Rxing),
            want,
            "{scan}"
        );
    }
}

#[test]
fn a_page_with_nothing_reads_nothing() {
    let nothing = PageCodes {
        patch: None,
        code: None,
    };
    for (name, page) in [
        ("blank", blank_page()),
        ("text", text_page(42, None)),
        ("rules", rules_page()),
    ] {
        for (scan, dpi, image) in at_300_and_150(&page) {
            let found = read(&image, dpi, &Symbology::ALL, &|_| true, &Rxing);
            assert_eq!(found, nothing, "{name} page, {scan}");
        }
    }
}
