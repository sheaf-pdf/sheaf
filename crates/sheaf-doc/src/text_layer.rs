//! OCR text layers.
//!
//! A [`TextLayer`] is the words an OCR engine recognized on one page. It
//! lives in [`DocState`](crate::DocState) until the document is saved, when
//! it is written into the page as an invisible text layer (text render mode
//! 3) so PDFium, Acrobat and everything else can search and select it.
//! Until then a page's realization draws it the same way, so Find and
//! selection read it from there (M10.5 §10.5).
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

use lopdf::{Dictionary, Object, Stream, dictionary};

use crate::annot::{Box2, Pt};
use crate::content::sink::{self, Sink};
use crate::content::{ContentWriter, Font, Glyphless, Res, Reserved, ResourceNeeds, TextMode};
use crate::geom::{Affine, Geom};

pub use sheaf_edit::{OcrWord, TextLayer};

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

/// Resource name of the layer's font on a page, until slice 18's blocks.
pub(crate) const FONT_NAME: &str = "SheafOcr";
/// Marks the content streams we add, so a re-save can find and replace them.
const MARKER: &str = "SheafOcrLayer";

/// The content stream for `layer`: invisible text, one `Tj` per word,
/// fitted to the word's box by horizontal scaling, in [`Glyphless`] named
/// [`FONT_NAME`]; and the font it needs. Text runs along the upright
/// frame's x, so it's turned by [`TextLayer::upright`]. Starts with `Q` to
/// close the `q` that precedes the original content.
pub(crate) fn to_content(layer: &TextLayer) -> (Vec<u8>, ResourceNeeds) {
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
    w.restore();
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
    w.finish(
        &Reserved::default(),
        &[(Glyphless.resource().id, FONT_NAME)],
    )
}

/// Add `layer` to `page` (an owned, flattened page dictionary about to be
/// written), read and written through `sink`: `/Contents` becomes `[q,
/// original…, layer]` and `/Resources` is inlined with the layer's font,
/// the `q` and the font written once a save. A layer of ours the page
/// holds already ([`has_layer`]) is taken out first, so a re-OCR leaves
/// one. An empty layer only takes the old one out.
pub(crate) fn install(sink: &mut dyn Sink, page: &mut Dictionary, layer: &TextLayer) {
    if has_layer(sink, page) {
        strip(sink, page);
    }
    if layer.is_empty() {
        return;
    }
    let (content, needs) = to_content(layer);
    let q = Stream::new(dictionary! { MARKER => true }, b"q\n".to_vec());
    let q = sink.resource(&Res::of([q.into()]));
    let mut stream = Stream::new(dictionary! { MARKER => true }, content);
    let _ = stream.compress();
    let ocr = sink.add(stream.into());
    // Never decode the original streams: array entries are concatenated
    // with whitespace, and the q/Q pair isolates whatever state they leave.
    let mut entries = vec![Object::Reference(q)];
    entries.extend(sink::content_entries(sink, page));
    entries.push(Object::Reference(ocr));
    page.set("Contents", Object::Array(entries));
    let mut resources = sink::resources(sink, page);
    sink::bind(sink, &needs, &mut resources);
    page.set("Resources", Object::Dictionary(resources));
}

/// Whether the page has an OCR layer of ours in its content: a stream
/// marked as one in its `/Contents` array.
pub(crate) fn has_layer(sink: &mut dyn Sink, page: &Dictionary) -> bool {
    layer_entries(sink, page).is_some_and(|(_, ours)| ours.contains(&true))
}

/// Remove a layer we wrote earlier from the page's `/Contents`.
pub(crate) fn strip(sink: &mut dyn Sink, page: &mut Dictionary) {
    let Some((entries, ours)) = layer_entries(sink, page) else {
        return;
    };
    if !ours.contains(&true) {
        return;
    }
    let kept: Vec<Object> = (entries.into_iter().zip(ours))
        .filter_map(|(e, ours)| (!ours).then_some(e))
        .collect();
    match kept.len() {
        0 => {
            page.remove(b"Contents");
        }
        1 => page.set("Contents", kept[0].clone()),
        _ => page.set("Contents", Object::Array(kept)),
    }
}

/// The page's `/Contents` array, when it has one, and which of its
/// entries are a layer's streams.
fn layer_entries(sink: &mut dyn Sink, page: &Dictionary) -> Option<(Vec<Object>, Vec<bool>)> {
    let entries = match page.get(b"Contents").ok()? {
        Object::Array(entries) => entries.clone(),
        Object::Reference(r) => sink.array(*r)?,
        _ => return None,
    };
    let ours = (entries.iter())
        .map(|e| {
            (e.as_reference().ok())
                .and_then(|r| sink.stream_dict(r))
                .is_some_and(|d| d.has(MARKER.as_bytes()))
        })
        .collect();
    Some((entries, ours))
}

#[cfg(test)]
mod tests {
    use lopdf::Document as Pdf;
    use sheaf_edit::Rotation;

    use super::*;
    use crate::content::sink::Lopdf;
    use crate::content::{self, ResObjects};
    use crate::geom::Rect;

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

    #[test]
    fn content_places_invisible_fitted_text() {
        let l = layer(
            vec![
                word("Hello", 72.0, 100.0, 40.0, 10.0),
                word("World", 122.0, 100.0, 40.0, 10.0),
            ],
            Rotation::R0,
        );
        let (c, needs) = to_content(&l);
        let c = String::from_utf8(c).unwrap();
        assert!(c.starts_with("Q\nBT\n3 Tr\n"), "{c}");
        assert!(c.contains("/SheafOcr 10 Tf\n"), "{c}");
        // 5 code units, size 10, width 40 → 200*40/(10*5) = 160.
        assert!(
            c.contains("1 0 0 1 72 682 Tm\n160 Tz\n<00480065006C006C006F> Tj\n"),
            "{c}"
        );
        // The 10 pt gap becomes a space scaled to fit it.
        assert!(c.contains("1 0 0 1 112 682 Tm\n200 Tz\n<0020> Tj\n"), "{c}");
        assert!(c.ends_with("ET\n"));
        assert_eq!(c.matches(" Tf").count(), 1, "font size emitted once");
        let [font] = &needs.0[..] else {
            panic!("{needs:?}")
        };
        assert_eq!(font.name, b"SheafOcr");
        assert_eq!(font.res, Glyphless.resource().into());
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
            to_content(&layer(words, Rotation::R90))
        }
        let first = write();
        assert_eq!(write(), first);
        let threads: Vec<_> = (0..2).map(|_| std::thread::spawn(write)).collect();
        for t in threads {
            assert_eq!(t.join().unwrap(), first);
        }
    }

    #[test]
    fn content_turns_with_the_upright_frame() {
        let matrix = |geom: Geom| {
            let l = from_engine(vec![word("Hi", 10.0, 20.0, 20.0, 10.0)], &geom, "t");
            let c = String::from_utf8(to_content(&l).0).unwrap();
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

    fn sink<'a>(pdf: &'a mut Pdf, res: &'a mut ResObjects) -> Lopdf<'a> {
        Lopdf { pdf, res }
    }

    #[test]
    fn install_and_strip_are_idempotent() {
        let mut pdf = Pdf::with_version("1.5");
        let orig = pdf.add_object(Stream::new(dictionary! {}, b"0 0 m".to_vec()));
        let font = pdf.add_object(dictionary! { "Type" => "Font" });
        let res = pdf.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
        let mut page = dictionary! { "Type" => "Page", "Contents" => orig, "Resources" => res };
        let l = layer(vec![word("Hi", 10.0, 20.0, 20.0, 10.0)], Rotation::R0);
        let mut objs = ResObjects::default();
        install(&mut sink(&mut pdf, &mut objs), &mut page, &l);
        let contents = page.get(b"Contents").unwrap().as_array().unwrap().clone();
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[1], Object::Reference(orig));
        let fonts = page
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Font")
            .unwrap()
            .as_dict()
            .unwrap()
            .clone();
        assert!(fonts.has(b"F1"));
        assert_eq!(
            fonts.get(b"SheafOcr").unwrap(),
            &Object::Reference(objs.object(&mut pdf, &Glyphless.resource().into()))
        );
        // A second install replaces rather than stacks.
        let before = pdf.objects.len();
        assert!(has_layer(&mut sink(&mut pdf, &mut objs), &page));
        install(&mut sink(&mut pdf, &mut objs), &mut page, &l);
        assert_eq!(page.get(b"Contents").unwrap().as_array().unwrap().len(), 3);
        assert_eq!(pdf.objects.len(), before + 1, "only a new content stream");
        strip(&mut sink(&mut pdf, &mut objs), &mut page);
        assert_eq!(page.get(b"Contents").unwrap(), &Object::Reference(orig));
        assert!(!has_layer(&mut sink(&mut pdf, &mut objs), &page));
        // An empty layer leaves the page alone.
        let empty = layer(vec![], Rotation::R0);
        install(&mut sink(&mut pdf, &mut objs), &mut page, &empty);
        assert_eq!(page.get(b"Contents").unwrap(), &Object::Reference(orig));
    }

    #[test]
    fn font_objects_match_tesseract_layout() {
        let mut pdf = Pdf::with_version("1.5");
        let l = layer(vec![word("Hi", 10.0, 20.0, 20.0, 10.0)], Rotation::R0);
        let mut res = ResObjects::default();
        let mut pages = [dictionary! {}, dictionary! {}];
        for page in &mut pages {
            install(&mut sink(&mut pdf, &mut res), page, &l);
        }
        let font = |page: &Dictionary| {
            let resources = page.get(b"Resources").unwrap().as_dict().unwrap();
            let fonts = resources.get(b"Font").unwrap().as_dict().unwrap();
            fonts.get(b"SheafOcr").unwrap().as_reference().unwrap()
        };
        let q = |page: &Dictionary| content::contents(page)[0];
        assert_eq!(font(&pages[0]), font(&pages[1]), "one font a save");
        assert_eq!(q(&pages[0]), q(&pages[1]), "one q a save");
        let type0 = pdf.get_dictionary(font(&pages[0])).unwrap();
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
        let q = pdf.get_object(q(&pages[0])).unwrap().as_stream().unwrap();
        assert_eq!(q.content, b"q\n");
        assert!(q.dict.has(MARKER.as_bytes()));
    }
}
