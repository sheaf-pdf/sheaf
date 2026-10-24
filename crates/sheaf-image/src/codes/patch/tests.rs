//! Patch codes laid out as Kodak's label and sheets and Alliance's sheet
//! lay them out, scanned every way (`fixture::scanned_every_way`) and the
//! bench's ways, read as their types; a turned label read from the edge it
//! came to; and pages with no patch, among them a feeder's black border, a
//! letterhead's rule, bars too short for a patch, bars without light
//! round them and bars too few lines cross, read as none. Then the type
//! chosen among several, the Otsu level, and a page too big for
//! imageproc's.

use image::{GrayImage, Luma};

use super::*;
use crate::codes::fixture::Degradation::*;
use crate::codes::fixture::*;
use crate::codes::{Code, Reader, Rxing, Symbology};

/// Reads each of `codes` laid out as `layout`, scanned every way, as its
/// type: every miss is listed, so a run shows them all.
fn read_scanned_every_way(layout: PatchLayout, codes: &[PatchCode]) {
    let mut misses = Vec::new();
    let mut reads = 0;
    for &code in codes {
        for scan in scanned_every_way(&patch_page(code, layout)) {
            reads += 1;
            let dpi = scan.dpi as f32;
            let found = patch(&scan.image, dpi);
            if found != Some(code) {
                let all = patches(&scan.image, dpi);
                misses.push(format!("{code:?}, {}: {found:?} {all:?}", scan.label));
            }
        }
    }
    assert!(
        misses.is_empty(),
        "{} of {reads} reads missed:\n{}",
        misses.len(),
        misses.join("\n")
    );
}

#[test]
fn kodak_label_patches_read_as_their_types_scanned_every_way() {
    read_scanned_every_way(PatchLayout::KodakLabel, &PatchCode::ALL);
}

#[test]
fn kodak_sheet_patches_read_as_their_types_scanned_every_way() {
    read_scanned_every_way(PatchLayout::KodakSheet, &PatchCode::ALL);
}

#[test]
fn alliance_t_reads_as_t_scanned_every_way() {
    read_scanned_every_way(PatchLayout::AllianceT, &[PatchCode::T]);
}

#[test]
fn the_bench_s_degradations_keep_patch_types() {
    let mut misses = Vec::new();
    for code in PatchCode::ALL {
        let page = patch_page(code, PatchLayout::KodakLabel);
        for (name, steps) in BENCH {
            let (scan, dpi) = degrade(&page, 300, steps);
            let found = patch(&scan, dpi as f32);
            if found != Some(code) {
                misses.push(format!("{code:?}, {name}: {found:?}"));
            }
        }
    }
    assert!(
        misses.is_empty(),
        "{} of {} reads missed:\n{}",
        misses.len(),
        PatchCode::ALL.len() * BENCH.len(),
        misses.join("\n")
    );
}

#[test]
fn patch_sheets_carry_their_type_as_code_39() {
    for (code, layout, text) in [
        (PatchCode::Three, PatchLayout::KodakSheet, "PATCH3"),
        (PatchCode::T, PatchLayout::AllianceT, "PATCHT"),
    ] {
        let (scan, _) = degrade(&patch_page(code, layout), 300, &[Dpi(200), Threshold(128)]);
        let found = Rxing.read(&scan, &[Symbology::Code39], &|_| true);
        let want = Code {
            symbology: Symbology::Code39,
            text: text.into(),
        };
        assert_eq!(found, Some(want), "{layout:?}");
    }
}

#[test]
fn turned_labels_tell_3_from_t_and_1_from_6() {
    // Kodak's label is along one edge, so a turn moves it to another, and
    // its bars read backwards from the edge it started at: 3 as T, 1 as 6.
    // Read from the edge they're along, they read as they were printed.
    let mut wrong = Vec::new();
    for code in [PatchCode::Three, PatchCode::One] {
        let page = patch_page(code, PatchLayout::KodakLabel);
        for (turn, edge) in [
            (&[][..], Edge::Top),
            (&[Turn(90)], Edge::Right),
            (&[Turn(180)], Edge::Bottom),
            (&[Turn(270)], Edge::Left),
        ] {
            let (turned, _) = degrade(&page, 300, turn);
            for dpi in [300, 200, 150] {
                let (gray, _) = degrade(&turned, 300, &[Dpi(dpi)]);
                let (bw, _) = degrade(&gray, dpi, &[Threshold(128)]);
                for (tone, scan) in [("gray", gray), ("1-bit", bw)] {
                    let found = patches(&scan, dpi as f32);
                    // The label's bars: 0.5 in in, 2.5 in long.
                    let as_drawn = |p: &Patch| {
                        p.code == code
                            && p.edge == edge
                            && (p.inset - 0.5).abs() < 0.05
                            && (2.25..=2.55).contains(&p.length)
                    };
                    if found.len() != 1 || !as_drawn(&found[0]) {
                        wrong.push(format!(
                            "{code:?} turned {turn:?}, {dpi} dpi {tone}: {found:?}"
                        ));
                    }
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn bars_shorter_than_a_patch_read_as_none() {
    // A-61599's bars are at least 0.75 in long; shorter ones, as in a logo
    // or a line of heavy type, aren't a patch.
    let mut wrong = Vec::new();
    for (long, want) in [(0.4, None), (0.75, Some(PatchCode::T))] {
        let mut page = patch_page(PatchCode::T, PatchLayout::KodakLabel);
        // The label's bars run from 3 to 5.5 in along the top, 0.5 to 1.3
        // in down: all but `long` of them whited out.
        for y in 120..420 {
            for x in ((3.0 + long) * 300.0) as u32..1700 {
                page.put_pixel(x, y, Luma([255]));
            }
        }
        for dpi in [300, 200, 150] {
            let (gray, _) = degrade(&page, 300, &[Dpi(dpi)]);
            let (bw, _) = degrade(&gray, dpi, &[Threshold(128)]);
            for (tone, scan) in [("gray", gray), ("1-bit", bw)] {
                let found = patch(&scan, dpi as f32);
                if found != want {
                    wrong.push(format!("{long} in, {dpi} dpi {tone}: {found:?}"));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn pages_without_patches_read_as_none() {
    let mut pages = vec![
        ("blank".to_string(), blank_page()),
        ("text".into(), text_page(42, None)),
        ("rules".into(), rules_page()),
        ("black border".into(), bordered_page()),
        ("letterhead rule".into(), edge_rule_page()),
    ];
    for (symbology, text, module_px, row_px) in SAMPLES {
        let page = code_page(symbology, text, module_px, row_px);
        pages.push((format!("{symbology:?}"), page));
    }
    let mut found = Vec::new();
    let mut reads = 0;
    for (name, page) in &pages {
        for (skew, steps) in [
            ("as is", &[][..]),
            ("skewed +3°", &[Skew(3.0)]),
            ("skewed -3°", &[Skew(-3.0)]),
        ] {
            let (skewed, _) = degrade(page, 300, steps);
            for dpi in [300, 200, 150] {
                let (gray, _) = degrade(&skewed, 300, &[Dpi(dpi)]);
                let (bw, _) = degrade(&gray, dpi, &[Threshold(128)]);
                for (tone, scan) in [("gray", gray), ("1-bit", bw)] {
                    reads += 1;
                    let patches = patches(&scan, dpi as f32);
                    if !patches.is_empty() {
                        found.push(format!(
                            "{name} page, {dpi} dpi {tone}, {skew}: {patches:?}"
                        ));
                    }
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} of {reads} pages read a patch:\n{}",
        found.len(),
        found.join("\n")
    );
}

/// Dark in `page`, at `dpi`, from `d0` to `d1` in from the top and from
/// `a0` to `a1` along it, in inches, in a patch bar's gray.
fn bar_at(page: &mut GrayImage, dpi: f32, (d0, d1): (f32, f32), (a0, a1): (f32, f32)) {
    let px = |inches: f32| (inches * dpi).round() as u32;
    for y in px(d0)..px(d1) {
        for x in px(a0)..px(a1) {
            page.put_pixel(x, y, Luma([10]));
        }
    }
}

/// Each of `pages` (300 dpi) at 300, 200 and 150 dpi, gray and 1-bit:
/// those that read a patch, listed.
fn read_any(pages: &[(&str, GrayImage)]) -> Vec<String> {
    let mut found = Vec::new();
    for (name, page) in pages {
        for dpi in [300, 200, 150] {
            let (gray, _) = degrade(page, 300, &[Dpi(dpi)]);
            let (bw, _) = degrade(&gray, dpi, &[Threshold(128)]);
            for (tone, scan) in [("gray", gray), ("1-bit", bw)] {
                let patches = patches(&scan, dpi as f32);
                if !patches.is_empty() {
                    found.push(format!("{name}, {dpi} dpi {tone}: {patches:?}"));
                }
            }
        }
    }
    found
}

#[test]
fn bars_without_light_round_them_read_as_none() {
    // A line reads a patch only between runs of light wider than a space:
    // print 0.1 in after a label's last bar (A-61599 asks 0.25 in), or
    // bars of a patch's widths that go on past four, aren't a patch.
    let mut printed = patch_page(PatchCode::T, PatchLayout::KodakLabel);
    // The label's bars end 1.3 in down; a block of print 0.4 in deep
    // starts 0.1 in after them.
    bar_at(&mut printed, 300.0, (1.4, 1.8), (3.0, 5.5));
    // Seven bars, narrow and wide in turn, 0.08 in apart, 0.5 in down.
    let mut striped = blank_page();
    let mut depth = 0.5;
    for i in 0..7 {
        let width = if i % 2 == 0 { 0.08 } else { 0.20 };
        bar_at(&mut striped, 300.0, (depth, depth + width), (3.0, 5.5));
        depth += width + 0.08;
    }
    let found = read_any(&[("print after a label", printed), ("seven bars", striped)]);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn bars_too_few_lines_cross_read_as_none() {
    // A patch T's bars broken into dashes along the edge, drawn at the
    // reader's own resolution (75 dpi a work pixel) so that each dash is
    // one work pixel wide and lies on one of the lines (0.04 in, 3 work
    // pixels apart): seven dashes on every third line reach 0.72 in, past
    // the least length, but only seven lines agree; the same dashes on
    // every line make a patch.
    let mut wrong = Vec::new();
    for (every, want) in [(3, None), (1, Some(PatchCode::T))] {
        for dpi in [300, 150] {
            let factor = dpi / 75;
            let mut page = white_page(8 * dpi, 10 * dpi);
            let mut depth = 0.5;
            for wide in PatchCode::T.bars() {
                let width = if wide { 0.20 } else { 0.08 };
                for line in (0..=18).step_by(every) {
                    let x = (201 + 3 * line) * factor;
                    let (y0, y1) = (depth * dpi as f32, (depth + width) * dpi as f32);
                    for y in y0.round() as u32..y1.round() as u32 {
                        for x in x..x + factor {
                            page.put_pixel(x, y, Luma([10]));
                        }
                    }
                }
                depth += width + 0.08;
            }
            let (bw, _) = degrade(&page, dpi, &[Threshold(128)]);
            for (tone, scan) in [("gray", page), ("1-bit", bw)] {
                let found = patch(&scan, dpi as f32);
                if found != want {
                    let all = patches(&scan, dpi as f32);
                    wrong.push(format!("every {every}, {dpi} dpi {tone}: {all:?}"));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_type_read_along_the_most_length_is_the_page_s() {
    let at = |code, edge, length| Patch {
        code,
        edge,
        inset: 0.62,
        length,
    };
    assert_eq!(strongest(&[]), None);
    // T along two edges outweighs 3 along a longer one.
    let found = [
        at(PatchCode::Three, Edge::Left, 9.0),
        at(PatchCode::T, Edge::Top, 5.0),
        at(PatchCode::T, Edge::Bottom, 5.0),
    ];
    assert_eq!(strongest(&found), Some(PatchCode::T));
    let found = [
        at(PatchCode::T, Edge::Top, 5.0),
        at(PatchCode::Three, Edge::Left, 9.0),
    ];
    assert_eq!(strongest(&found), Some(PatchCode::Three));
    // A tie goes to the first of ALL, whatever the order found.
    let found = [
        at(PatchCode::Six, Edge::Top, 2.5),
        at(PatchCode::Two, Edge::Left, 2.5),
    ];
    assert_eq!(strongest(&found), Some(PatchCode::Two));
}

#[test]
fn the_otsu_level_is_imageproc_s() {
    let page = patch_page(PatchCode::Four, PatchLayout::KodakSheet);
    let (scan, _) = degrade(&page, 300, BENCH[BENCH.len() - 1].1);
    for image in [page, scan, blank_page(), white_page(0, 0)] {
        let reduced = reduced(&image, 4);
        assert_eq!(
            otsu_level(&reduced),
            imageproc::contrast::otsu_level(&reduced)
        );
    }
}

#[test]
fn a_page_too_big_for_imageproc_s_otsu_level_reads() {
    // 17.6 M white pixels at 75 dpi: more than imageproc's u32 sums hold.
    let page = white_page(4200, 4200);
    assert_eq!(patches(&page, 75.0), []);
}
