//! OCR text layers.
//!
//! A [`TextLayer`] is the words an OCR engine recognized on one page. It's
//! page content (M10.5 §3.4): a layer of the page's content stack, drawn
//! over its own content as a block of its own (§11.4), invisible text
//! (text render mode 3) that PDFium, Acrobat and everything else can
//! search and select. A page's realization draws it as a save writes it,
//! so Find and selection read it from there (§10.5). A layer in a file is
//! one like any other, saved or not: re-recognizing the page, or clearing
//! its text, takes it out (`StreamSel::SheafOcr`, `MarkedSel::SheafOcr`),
//! the layer an older sheaf wrote too.
//!
//! Word rects are in PageSpace (M10.5 §5), like marks, so cropping or
//! turning the page moves none of them. The frame the text reads upright
//! in is recorded as [`TextLayer::upright`], the page's rotation when it
//! was recognized; lines run along that frame's x, which the writer turns
//! the text to.
//!
//! The written layer is in [`Glyphless`], Tesseract's approach
//! (`pdfrenderer.cpp`): a Type0 font with an embedded glyphless TrueType
//! whose every CID advances half an em, so a word is fitted to its box by
//! horizontal scaling alone and any Unicode text round-trips.

use std::cmp::Ordering;
use std::ops::Range;

use lopdf::{Object, dictionary};

use crate::annot::{Box2, Pt};
use crate::content::{BlockId, ContentWriter, Glyphless, Reserved, ResourceNeeds, TextMode};
use crate::geom::{Affine, Geom};
use crate::materialize::Private;

pub use sheaf_edit::{OcrWord, TextLayer};

#[cfg(any(test, feature = "fixtures"))]
pub mod fixture;

/// A layer from words as the engine saw them: in points, in the view frame
/// of `geom`, the page as it was rendered. Words are grouped into lines
/// there, then stored in PageSpace.
pub fn from_engine(words: Vec<OcrWord>, geom: &Geom, engine: &str) -> TextLayer {
    let mut words: Vec<OcrWord> = words
        .into_iter()
        .filter(|w| !w.text.trim().is_empty() && w.rect.w > 0.0 && w.rect.h > 0.0)
        .collect();
    let lines = group_lines(&mut words);
    let from_view = geom.from_view();
    for w in &mut words {
        w.rect = from_view.apply_box(w.rect);
    }
    TextLayer {
        words,
        lines,
        upright: geom.rotate,
        engine: engine.to_string(),
    }
}

/// A word's box in the layer's upright frame (y down; where on the page
/// doesn't matter, only which way the text runs).
pub(crate) fn upright_box(layer: &TextLayer, w: &OcrWord) -> Box2 {
    Affine::upright(layer.upright).apply_box(w.rect)
}

/// Order `words` into lines (top to bottom, then left to right) and return
/// the index range of each line. A word joins the current line when its
/// vertical overlap with it is at least half the smaller height.
fn group_lines(words: &mut Vec<OcrWord>) -> Vec<Range<usize>> {
    let cmp = |a: f32, b: f32| a.partial_cmp(&b).unwrap_or(Ordering::Equal);
    let mut order: Vec<usize> = (0..words.len()).collect();
    order.sort_by(|&a, &b| {
        let (ra, rb) = (&words[a].rect, &words[b].rect);
        cmp(ra.y + ra.h / 2.0, rb.y + rb.h / 2.0).then(cmp(ra.x, rb.x))
    });
    let mut lines: Vec<(Box2, Vec<usize>)> = Vec::new();
    for i in order {
        let r = words[i].rect;
        match lines.last_mut() {
            Some((lb, members)) if overlaps_line(*lb, r) => {
                *lb = lb.union(r);
                members.push(i);
            }
            _ => lines.push((r, vec![i])),
        }
    }
    let mut out = Vec::with_capacity(words.len());
    let mut ranges = Vec::with_capacity(lines.len());
    for (_, mut members) in lines {
        members.sort_by(|&a, &b| cmp(words[a].rect.x, words[b].rect.x));
        let start = out.len();
        for i in members {
            out.push(words[i].clone());
        }
        ranges.push(start..out.len());
    }
    *words = out;
    ranges
}

/// Whether `r` is level with a line whose box is `line`: their vertical
/// overlap is at least half the smaller height.
pub(crate) fn overlaps_line(line: Box2, r: Box2) -> bool {
    let overlap = line.bottom().min(r.bottom()) - line.y.max(r.y);
    overlap >= 0.5 * line.h.min(r.h)
}

// ----- PDF writing ------------------------------------------------------------

/// Layer `layer`, block `id`'s, as its block draws it (§11.4): invisible
/// text, one `Tj` per word, fitted to the word's box by horizontal scaling,
/// in [`Glyphless`]; marked `/ShOCR` with a property list that names its
/// block (`<< /SHEF_OcrBlock (b…) >>`), so a stream another tool merges it
/// into still says what it is, or with none when `private` strips that
/// (§15); and in a `q` of its own, so the text mode it sets reaches
/// nothing drawn after it. Text runs along the upright frame's x, so it's
/// turned by [`TextLayer::upright`]. The names it draws with, the font's
/// and the list's, are kept clear of `reserved`.
pub(crate) fn block(
    layer: &TextLayer,
    id: BlockId,
    reserved: &Reserved,
    private: Private,
) -> (Vec<u8>, ResourceNeeds) {
    let to_user = Affine::upright(layer.upright).inverse();
    let theta = layer.upright.degrees() as f32;
    let (cs, sn) = (theta.to_radians().cos(), theta.to_radians().sin());
    let tm = |x: f32, y: f32| {
        let o = to_user.apply(Pt { x, y });
        Affine {
            a: cs,
            b: sn,
            c: -sn,
            d: cs,
            e: o.x,
            f: o.y,
        }
    };
    let mut w = ContentWriter::new();
    match private {
        Private::Keep => {
            let list = dictionary! { "SHEF_OcrBlock" => Object::string_literal(id.to_string()) };
            w.begin_marked("ShOCR", list);
        }
        Private::Strip | Private::Scrub => w.begin_tagged("ShOCR"),
    }
    w.save();
    w.begin_text();
    w.text_mode(TextMode::Invisible);
    let mut last_fs: Option<f32> = None;
    for line in &layer.lines {
        let mut prev_right: Option<f32> = None;
        for word in &layer.words[line.clone()] {
            let r = upright_box(layer, word);
            let n = word.text.encode_utf16().count();
            if n == 0 || r.h < 0.1 || r.w <= 0.0 {
                prev_right = None;
                continue;
            }
            let fs = r.h;
            if last_fs.is_none_or(|f| (fs - f).abs() > 1e-3) {
                w.font(&Glyphless, fs);
                last_fs = Some(fs);
            }
            // The gap to the previous word becomes a real space so viewers
            // separate the words.
            if let Some(pr) = prev_right {
                let gap = r.x - pr;
                if gap > 0.1 {
                    w.text_matrix(tm(pr, r.bottom()));
                    w.horizontal_scale(200.0 * gap / fs);
                    w.text(" ");
                }
            }
            // Each CID advances half an em, so scale n half-ems to the box.
            w.text_matrix(tm(r.x, r.bottom()));
            w.horizontal_scale(200.0 * r.w / (fs * n as f32));
            w.text(&word.text);
            prev_right = Some(r.right());
        }
    }
    w.end_text();
    w.restore();
    w.end_marked();
    w.finish(reserved)
}

#[cfg(test)]
mod tests {
    use lopdf::Document as Pdf;
    use sheaf_edit::Rotation;

    use super::*;
    use crate::content::{Font, Need, Res, ResKind};
    use crate::geom::Rect;
    use crate::serialize::built::ResObjects;

    const ROTATIONS: [Rotation; 4] = [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270];

    fn word(text: &str, x: f32, y: f32, w: f32, h: f32) -> OcrWord {
        OcrWord {
            text: text.into(),
            rect: Box2 { x, y, w, h },
            confidence: 0.9,
        }
    }

    /// A Letter page turned `r`.
    fn letter(r: Rotation) -> Geom {
        Geom::read(None, None, r.degrees() as i64)
    }

    /// A layer of `words` as the engine saw them on a Letter page turned
    /// `upright`.
    fn layer(words: Vec<OcrWord>, upright: Rotation) -> TextLayer {
        from_engine(words, &letter(upright), "test")
    }

    fn two_lines() -> TextLayer {
        layer(
            vec![
                word("World", 130.0, 100.0, 50.0, 10.0),
                word("Hello", 72.0, 100.0, 50.0, 10.0),
                word("second", 72.0, 120.0, 60.0, 10.0),
                word("line", 140.0, 121.0, 30.0, 9.0),
            ],
            Rotation::R0,
        )
    }

    #[test]
    fn groups_and_orders_lines() {
        let l = two_lines();
        let texts: Vec<&str> = l.words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, ["Hello", "World", "second", "line"]);
        assert_eq!(l.lines, vec![0..2, 2..4]);
        assert!(l.byte_estimate() > 0);
    }

    #[test]
    fn drops_blank_and_degenerate_words() {
        let l = layer(
            vec![
                word("  ", 0.0, 0.0, 10.0, 10.0),
                word("x", 0.0, 0.0, 0.0, 10.0),
            ],
            Rotation::R0,
        );
        assert!(l.is_empty());
    }

    /// A word the engine saw on a page shown at any rotation, with boxes
    /// off the origin, is stored in PageSpace and comes back through the
    /// page's `to_view` to where the engine saw it; in the upright frame
    /// it keeps its size and runs the same way.
    #[test]
    fn words_come_back_through_to_view_for_every_frame() {
        let seen = Box2 {
            x: 10.0,
            y: 20.0,
            w: 30.0,
            h: 5.0,
        };
        let media = Rect::new(-37.0, 52.0, 575.0, 844.0);
        let crop = Rect::new(-17.0, 62.0, 555.0, 824.0);
        for r in ROTATIONS {
            let geom = Geom::read(Some(media), Some(crop), r.degrees() as i64);
            let l = from_engine(
                vec![word("w", seen.x, seen.y, seen.w, seen.h)],
                &geom,
                "test",
            );
            assert_eq!(l.upright, r);
            let back = geom.to_view().apply_box(l.words[0].rect);
            for (got, want) in [
                (back.x, seen.x),
                (back.y, seen.y),
                (back.w, seen.w),
                (back.h, seen.h),
            ] {
                assert!((got - want).abs() < 1e-3, "{r:?}: {back:?}");
            }
            let up = upright_box(&l, &l.words[0]);
            assert!(
                (up.w - seen.w).abs() < 1e-3 && (up.h - seen.h).abs() < 1e-3,
                "{r:?}: {up:?}"
            );
        }
        // On a Letter page turned 90°, the top-left as shown is PageSpace's
        // bottom-left.
        let l = layer(vec![word("w", 0.0, 0.0, 10.0, 10.0)], Rotation::R90);
        assert_eq!(
            l.words[0].rect,
            Box2 {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0
            }
        );
    }

    /// [`block`] of `l` with no names taken, as block 0x2a.
    fn written(l: &TextLayer) -> (Vec<u8>, ResourceNeeds) {
        block(l, BlockId(0x2a), &Reserved::default(), Private::Keep)
    }

    #[test]
    fn block_places_invisible_fitted_text() {
        let l = layer(
            vec![
                word("Hello", 72.0, 100.0, 40.0, 10.0),
                word("World", 122.0, 100.0, 40.0, 10.0),
            ],
            Rotation::R0,
        );
        let (c, needs) = written(&l);
        let c = String::from_utf8(c).unwrap();
        let [list, font] = &needs.0[..] else {
            panic!("{needs:?}")
        };
        let name = |n: &Need| String::from_utf8(n.name.clone()).unwrap();
        // Marked as the block's, its text invisible in a `q` of its own.
        let start = format!("/ShOCR /{} BDC\nq\nBT\n3 Tr\n", name(list));
        assert!(c.starts_with(&start), "{c}");
        assert!(c.ends_with("ET\nQ\nEMC\n"), "{c}");
        assert_eq!(list.kind, ResKind::Properties);
        let said = dictionary! { "SHEF_OcrBlock" => Object::string_literal("b2a") };
        assert_eq!(list.res, Res::dict(said));
        assert!(c.contains(&format!("/{} 10 Tf\n", name(font))), "{c}");
        assert!(font.name.starts_with(b"Sh"));
        assert_eq!(font.res, Glyphless.resource().into());
        // 5 code units, size 10, width 40 → 200*40/(10*5) = 160.
        assert!(
            c.contains("1 0 0 1 72 682 Tm\n160 Tz\n<00480065006C006C006F> Tj\n"),
            "{c}"
        );
        // The 10 pt gap becomes a space scaled to fit it.
        assert!(c.contains("1 0 0 1 112 682 Tm\n200 Tz\n<0020> Tj\n"), "{c}");
        assert_eq!(c.matches(" Tf").count(), 1, "font size emitted once");
    }

    /// The fixtures' block is what [`block`] writes: its bytes frozen in
    /// [`fixture::OCR_BLOCK`], with the names it gives the font and list.
    #[test]
    fn the_fixture_block_is_what_block_writes() {
        let (c, needs) = block(
            &fixture::layer(),
            BlockId(fixture::BLOCK_ID),
            &Reserved::default(),
            Private::Keep,
        );
        assert_eq!(c, fixture::OCR_BLOCK, "{}", String::from_utf8_lossy(&c));
        let names: Vec<&[u8]> = needs.0.iter().map(|n| &n.name[..]).collect();
        assert_eq!(names, [fixture::OCR_LIST, fixture::OCR_FONT]);
    }

    /// Without sheaf's private data, a block is marked `/ShOCR` with no
    /// list, and draws its words as it does with one.
    #[test]
    fn a_block_without_private_data_has_no_list() {
        let (l, id) = (fixture::layer(), BlockId(fixture::BLOCK_ID));
        let (kept, _) = block(&l, id, &Reserved::default(), Private::Keep);
        let (stripped, needs) = block(&l, id, &Reserved::default(), Private::Strip);
        assert!(stripped.starts_with(b"/ShOCR BMC\nq\n"));
        let kinds: Vec<ResKind> = needs.0.iter().map(|n| n.kind).collect();
        assert_eq!(kinds, [ResKind::Font]);
        let words = |c: &[u8]| c[c.windows(2).position(|w| w == b"q\n").unwrap()..].to_vec();
        assert_eq!(words(&stripped), words(&kept));
    }

    /// The names a block draws with keep clear of those the page binds to
    /// something else.
    #[test]
    fn block_names_keep_clear_of_the_page_s() {
        let (_, needs) = written(&two_lines());
        let mut reserved = Reserved::default();
        reserved.take(&ResourceNeeds(
            (needs.0.iter())
                .map(|n| Need {
                    res: Res::dict(dictionary! { "Other" => 1 }),
                    ..n.clone()
                })
                .collect(),
        ));
        let (_, clear) = block(&two_lines(), BlockId(0x2a), &reserved, Private::Keep);
        for (was, now) in needs.0.iter().zip(&clear.0) {
            assert_eq!((was.kind, &was.res), (now.kind, &now.res));
            assert_ne!(was.name, now.name);
        }
    }

    /// The text runs along the upright frame's x: turned with the page it
    /// was recognized on, starting at its word's bottom-left as it read.
    /// The same layer gives the same bytes and names, twice and on two
    /// threads.
    #[test]
    fn identical_layers_write_identical_bytes() {
        fn write() -> (Vec<u8>, ResourceNeeds) {
            let words = vec![
                word("Hello", 72.0, 100.0, 50.0, 10.0),
                word("wörld", 130.0, 100.0, 50.0, 10.0),
                word("日本", 72.0, 120.0, 30.0, 10.0),
            ];
            written(&layer(words, Rotation::R90))
        }
        let first = write();
        assert_eq!(write(), first);
        let threads: Vec<_> = (0..2).map(|_| std::thread::spawn(write)).collect();
        for t in threads {
            assert_eq!(t.join().unwrap(), first);
        }
    }

    #[test]
    fn block_turns_with_the_upright_frame() {
        let matrix = |geom: Geom| {
            let l = from_engine(vec![word("Hi", 10.0, 20.0, 20.0, 10.0)], &geom, "t");
            let c = String::from_utf8(written(&l).0).unwrap();
            let tm = c.lines().find(|l| l.ends_with(" Tm")).unwrap();
            tm.trim_end_matches(" Tm").to_string()
        };
        let turned = |r| matrix(letter(r));
        // Upright: the word's bottom-left, (10, 30) down the page, is
        // (10, 762) up it.
        assert_eq!(turned(Rotation::R0), "1 0 0 1 10 762");
        // Turned 90°: (10, 30) as shown is user (30, 10).
        assert_eq!(turned(Rotation::R90), "0 1 -1 0 30 10");
        assert!(turned(Rotation::R180).starts_with("-1 0 0 -1 "));
        assert!(turned(Rotation::R270).starts_with("0 -1 1 0 "));
        // A media box off the origin moves the text with it.
        let media = Rect::new(-37.0, 52.0, 575.0, 844.0);
        let off = matrix(Geom::read(Some(media), None, 0));
        assert_eq!(off, "1 0 0 1 -27 814");
    }

    /// The font a block draws in is Tesseract's glyphless Type0, one
    /// resource however many blocks draw in it, written once a save.
    #[test]
    fn font_objects_match_tesseract_layout() {
        let mut pdf = Pdf::with_version("1.5");
        let font = |l: &TextLayer| {
            let (_, needs) = written(l);
            let font = needs.0.into_iter().find(|n| n.kind == ResKind::Font);
            font.unwrap().res
        };
        let mut res = ResObjects::default();
        let one = layer(vec![word("Hi", 10.0, 20.0, 20.0, 10.0)], Rotation::R0);
        let first = res.object(&mut pdf, &font(&one));
        let second = res.object(&mut pdf, &font(&two_lines()));
        assert_eq!(first, second, "one font a save");
        let type0 = pdf.get_dictionary(first).unwrap();
        assert_eq!(
            type0.get(b"Encoding").unwrap().as_name().unwrap(),
            b"Identity-H"
        );
        let cid = pdf
            .get_dictionary(
                type0.get(b"DescendantFonts").unwrap().as_array().unwrap()[0]
                    .as_reference()
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(cid.get(b"DW").unwrap().as_i64().unwrap(), 500);
        let c2g = pdf
            .get_object(cid.get(b"CIDToGIDMap").unwrap().as_reference().unwrap())
            .unwrap()
            .as_stream()
            .unwrap();
        let map = c2g.decompressed_content().unwrap();
        assert_eq!(map.len(), 131072);
        assert_eq!(&map[..4], &[0, 1, 0, 1]);
        let desc = pdf
            .get_dictionary(cid.get(b"FontDescriptor").unwrap().as_reference().unwrap())
            .unwrap();
        let ff2 = pdf
            .get_object(desc.get(b"FontFile2").unwrap().as_reference().unwrap())
            .unwrap()
            .as_stream()
            .unwrap();
        assert_eq!(ff2.decompressed_content().unwrap(), Glyphless::PROGRAM);
        let cmap = pdf
            .get_object(type0.get(b"ToUnicode").unwrap().as_reference().unwrap())
            .unwrap()
            .as_stream()
            .unwrap();
        let cmap = String::from_utf8(cmap.decompressed_content().unwrap()).unwrap();
        assert!(cmap.contains("beginbfrange"));
    }
}
