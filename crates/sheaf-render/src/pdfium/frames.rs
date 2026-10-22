//! `Geom`'s frame against PDFium's (M10.5 §5): over Greg's corpus, a
//! page's `to_view` is PDFium's page-to-device mapping, on the pages a
//! file turns each widget lands where PDFium puts it, and the characters'
//! PageSpace boxes show where PDFium shows them (slice 14). sheaf has no
//! widget hit test of its own yet (M10 slice 10 adds one), so PDFium's
//! mapping is the one to match. Skips without the corpus, but for a
//! fixture's characters.

use sheaf_doc::forms::corpus::{Corpus, Entry, each_entry};
use sheaf_doc::geom::Geom;
use sheaf_doc::store::BlobKind;
use sheaf_doc::{Pt, Src, Store};
use sheaf_edit::Rotation;

use super::*;

/// Device pixels per point PDFium maps at, so its whole pixels are
/// sixteenths of a point.
const SCALE: f32 = 16.0;

/// Within a pixel at [`SCALE`], which covers PDFium's rounding to whole
/// pixels.
fn near(a: Pt, b: Pt) -> bool {
    (a.x - b.x).abs() <= 1.0 / SCALE && (a.y - b.y).abs() <= 1.0 / SCALE
}

/// PDFium's mappings between a page and a device the size it shows at
/// [`SCALE`].
struct Device<'a> {
    page: PdfPage<'a>,
    config: PdfRenderConfig,
    /// Pixels per point across and down: [`SCALE`], but for the device's
    /// size in whole pixels.
    scale: (f32, f32),
}

impl<'a> Device<'a> {
    /// Page `index` of `engine`, which shows `geom`; None when PDFium
    /// can't load it or nothing of it shows. Under [`lock`].
    fn of(engine: &'a PdfiumEngine, index: u32, geom: &Geom) -> Option<Device<'a>> {
        let shown = geom.shown();
        if shown.width < 1.0 || shown.height < 1.0 {
            return None;
        }
        let page = engine.doc.pages().get(index as i32).ok()?;
        let (w, h) = (
            (shown.width * SCALE).round(),
            (shown.height * SCALE).round(),
        );
        Some(Device {
            page,
            config: PdfRenderConfig::new().set_fixed_size(w as i32, h as i32),
            scale: (w / shown.width, h / shown.height),
        })
    }

    /// Where PDFium shows user-space point `p`, in points from the shown
    /// page's top-left.
    fn page_to_device(&self, p: Pt) -> Option<Pt> {
        let (x, y) = self
            .page
            .points_to_pixels(PdfPoints::new(p.x), PdfPoints::new(p.y), &self.config)
            .ok()?;
        Some(Pt {
            x: x as f32 / self.scale.0,
            y: y as f32 / self.scale.1,
        })
    }

    /// The user-space point PDFium shows at `v`.
    fn device_to_page(&self, v: Pt) -> Option<Pt> {
        let (x, y) = (
            (v.x * self.scale.0).round() as i32,
            (v.y * self.scale.1).round() as i32,
        );
        let (x, y) = self.page.pixels_to_points(x, y, &self.config).ok()?;
        Some(Pt {
            x: x.value,
            y: y.value,
        })
    }
}

/// A corpus file's bytes, and the file as a source; None when it doesn't
/// open as one.
fn source(corpus: &Corpus, entry: &Entry) -> Option<(Arc<[u8]>, Src)> {
    let path = corpus.path(entry);
    let bytes: Arc<[u8]> = std::fs::read(&path).ok()?.into();
    let src = Store::new()
        .open(bytes.clone(), BlobKind::file(&path, None))
        .ok()?;
    Some((bytes, src))
}

/// Whether a page's frame is more than the plain one, a y flip below the
/// height: its file turns it, or its visible box is off the origin.
fn moved(geom: &Geom) -> bool {
    let v = geom.visible();
    geom.rotate != Rotation::R0 || v.x0 != 0.0 || v.y0 != 0.0
}

/// How many of a corpus file's pages, `every` one or the [`moved`] ones,
/// PDFium shows, and where a corner of a page's visible box doesn't show
/// where `to_view` puts it.
fn corners(corpus: &Corpus, entry: &Entry, every: bool) -> Option<(usize, Vec<String>)> {
    let (bytes, src) = source(corpus, entry)?;
    let wanted: Vec<u32> = (0..src.page_count())
        .filter(|&i| every || moved(&src.geom(i)))
        .collect();
    if wanted.is_empty() {
        return Some((0, Vec::new()));
    }
    let engine = PdfiumEngine::open_bytes(bytes, None).ok()?;
    let _pdfium = lock();
    let (mut pages, mut differ) = (0, Vec::new());
    for index in wanted {
        let geom = src.geom(index);
        let Some(device) = Device::of(&engine, index, &geom) else {
            continue;
        };
        pages += 1;
        let v = geom.visible();
        for (x, y) in [(v.x0, v.y0), (v.x0, v.y1), (v.x1, v.y1), (v.x1, v.y0)] {
            let corner = Pt { x, y };
            let want = geom.to_view().apply(corner);
            match device.page_to_device(corner) {
                Some(got) if near(got, want) => {}
                got => differ.push(format!(
                    "page {index} ({geom:?}): {corner:?} at {want:?}, PDFium's {got:?}"
                )),
            }
        }
    }
    Some((pages, differ))
}

/// `check` over the corpus, each file giving how many `things` it
/// compared and where they differ: something is compared, and none
/// differ.
fn none_differ(
    things: &str,
    check: impl Fn(&Corpus, &Entry) -> Option<(usize, Vec<String>)> + Sync,
) {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let entries = corpus.forms().unwrap();
    let outcomes = each_entry(&entries, |entry| check(&corpus, entry));
    let compared: usize = outcomes.iter().flatten().map(|o| o.0).sum();
    eprintln!("{} files: {compared} {things} compared", entries.len());
    assert!(compared > 0, "no {things} were compared");
    let differ: Vec<(&str, &[String])> = entries
        .iter()
        .zip(&outcomes)
        .filter_map(|(entry, o)| {
            let found = &o.as_ref()?.1;
            (!found.is_empty()).then(|| (entry.path.as_str(), &found[..found.len().min(3)]))
        })
        .collect();
    assert!(differ.is_empty(), "{differ:#?}");
}

/// [`corners`] over the corpus: none differ, within 1/16 pt.
fn corners_are_pdfium_s(every: bool) {
    none_differ("pages", |corpus, entry| corners(corpus, entry, every));
}

/// The corpus pages whose frame is more than the plain one map their
/// visible box's corners to the page as shown as PDFium does. They're
/// about 400 of its 24,000 pages, which take PDFium 100 s to load.
#[test]
fn to_view_is_pdfium_s_page_to_device() {
    corners_are_pdfium_s(false);
}

/// [`to_view_is_pdfium_s_page_to_device`] on every page.
#[test]
#[ignore = "every corpus page, 100 s: cargo test -p sheaf-render -- --ignored to_view_is"]
fn to_view_is_pdfium_s_page_to_device_on_every_page() {
    corners_are_pdfium_s(true);
}

/// How many widgets are on the pages a corpus file turns, and which of
/// them PDFium doesn't put where `to_view` does, or doesn't find under
/// that point again.
fn turned_widgets(corpus: &Corpus, entry: &Entry) -> Option<(usize, Vec<String>)> {
    let (bytes, src) = source(corpus, entry)?;
    let turned = |index: &u32| src.geom(*index).rotate != Rotation::R0;
    if !(0..src.page_count()).any(|i| turned(&i)) {
        return Some((0, Vec::new()));
    }
    let form = sheaf_doc::forms::read(&src);
    let engine = PdfiumEngine::open_bytes(bytes, None).ok()?;
    let _pdfium = lock();
    let (mut widgets, mut differ) = (0, Vec::new());
    for (index, list) in form.widgets.iter().filter(|(i, _)| turned(i)) {
        let geom = src.geom(*index);
        let Some(device) = Device::of(&engine, *index, &geom) else {
            continue;
        };
        for w in list {
            widgets += 1;
            let [x0, y0, x1, y1] = w.rect;
            let centre = Pt {
                x: (x0 + x1) / 2.0,
                y: (y0 + y1) / 2.0,
            };
            let view = geom.to_view().apply(centre);
            let shown = device.page_to_device(centre);
            let back = device.device_to_page(view);
            let inside = |p: Pt| {
                let slack = 1.0 / SCALE;
                p.x >= x0 - slack && p.x <= x1 + slack && p.y >= y0 - slack && p.y <= y1 + slack
            };
            if !shown.is_some_and(|s| near(s, view)) || !back.is_some_and(inside) {
                differ.push(format!(
                    "page {index}, widget {:?} {:?}: at {view:?}, PDFium's {shown:?}, \
                     and back {back:?}",
                    w.object, w.rect
                ));
            }
        }
    }
    Some((widgets, differ))
}

/// On every page a corpus file turns, each widget's centre shows where
/// PDFium shows it, and PDFium's point under that place is in the
/// widget: a click there is on it.
#[test]
fn widgets_land_where_pdfium_puts_them() {
    none_differ("widgets on turned pages", turned_widgets);
}

/// How far from the page as shown, in points, a character's box is
/// compared: 2^17, where an f32 still holds 1/64 pt. Farther, the
/// comparison would measure f32 and PDFium's `i32` device pixels rather
/// than the frame: a corpus page draws characters 2^31 pt up, which
/// PDFium's device clamps.
const FAR: f32 = 131_072.0;

/// How many characters on `pages` of `src` have boxes, how many of those
/// are [`FAR`] off the page, and which of the rest, through the page's
/// `to_view`, don't show within 1/16 pt of where PDFium shows its own
/// loose box for that character.
fn char_boxes(
    engine: &PdfiumEngine,
    src: &Src,
    pages: impl IntoIterator<Item = u32>,
) -> (usize, usize, Vec<String>) {
    let (mut chars, mut far, mut differ) = (0, 0, Vec::new());
    for index in pages {
        let geom = src.geom(index);
        let Ok(read) = engine.page_chars(PageIndex(index)) else {
            continue;
        };
        let _pdfium = lock();
        let Some(device) = Device::of(engine, index, &geom) else {
            continue;
        };
        let Ok(text) = device.page.text() else {
            continue;
        };
        // PDFium's characters as `page_chars` reads them: those with a
        // code point, in its order.
        let all = text.chars();
        let theirs: Vec<PdfPageTextChar> = (all.iter())
            .filter(|c| c.unicode_char().is_some())
            .collect();
        if theirs.len() != read.len() {
            differ.push(format!(
                "page {index}: {} characters, PDFium's {}",
                read.len(),
                theirs.len()
            ));
            continue;
        }
        for (c, theirs) in read.iter().zip(&theirs) {
            let Some(r) = c.rect else { continue };
            chars += 1;
            let shows = geom.to_view().apply_box(r);
            if [shows.x, shows.y, shows.right(), shows.bottom()]
                .iter()
                .any(|v| v.abs() >= FAR)
            {
                far += 1;
                continue;
            }
            let bounds = theirs
                .loose_bounds()
                .ok()
                .filter(|_| theirs.unicode_char() == Some(c.ch));
            let Some(b) = bounds else {
                differ.push(format!(
                    "page {index}: {:?} isn't PDFium's {:?}",
                    c.ch,
                    theirs.unicode_char()
                ));
                continue;
            };
            let (left, bottom, right, top) = (
                b.left().value,
                b.bottom().value,
                b.right().value,
                b.top().value,
            );
            let pt = |x, y| Pt { x, y };
            let corners = [
                (pt(r.x, r.y), pt(left, bottom)),
                (pt(r.right(), r.y), pt(right, bottom)),
                (pt(r.x, r.bottom()), pt(left, top)),
                (pt(r.right(), r.bottom()), pt(right, top)),
            ];
            for (ours, pdfium_s) in corners {
                let want = geom.to_view().apply(ours);
                match device.page_to_device(pdfium_s) {
                    Some(got) if near(got, want) => {}
                    got => differ.push(format!(
                        "page {index} ({geom:?}), {:?} at {r:?}: {ours:?} shows at {want:?}, \
                         PDFium's {pdfium_s:?} at {got:?}",
                        c.ch
                    )),
                }
            }
        }
    }
    (chars, far, differ)
}

/// [`char_boxes`] on a corpus file's [`moved`] pages: how many characters
/// were compared, and where they differ.
fn corpus_char_boxes(corpus: &Corpus, entry: &Entry) -> Option<(usize, Vec<String>)> {
    let (bytes, src) = source(corpus, entry)?;
    let wanted: Vec<u32> = (0..src.page_count())
        .filter(|&i| moved(&src.geom(i)))
        .collect();
    if wanted.is_empty() {
        return Some((0, Vec::new()));
    }
    let engine = PdfiumEngine::open_bytes(bytes, None).ok()?;
    let (chars, far, differ) = char_boxes(&engine, &src, wanted);
    if far > 0 {
        eprintln!("{}: {far} characters far off the page", entry.path);
    }
    Some((chars - far, differ))
}

/// The text service's boxes are PDFium's own, in PageSpace, where the 16×
/// device trick had them in the page as shown (slice 14): on the corpus
/// pages whose frame is more than the plain one, every character's box,
/// shown through the page's `to_view`, is within 1/16 pt of where PDFium
/// shows its own. On a plain page the frame is a y flip, which
/// [`to_view_is_pdfium_s_page_to_device_on_every_page`] checks, so a box
/// built as PDFium gives it shows where PDFium shows it there too.
#[test]
fn char_boxes_lie_within_a_sixteenth_of_a_point_of_pdfium_s() {
    none_differ("characters", corpus_char_boxes);
}

/// A Letter page the file turns 90°, its crop box off the origin, with a
/// line of Helvetica across the page and one drawn up it.
fn turned_text() -> Vec<u8> {
    use lopdf::{Document, Stream, dictionary};
    let mut pdf = Document::with_version("1.5");
    let pages = pdf.new_object_id();
    let font = dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" };
    let content = b"BT /F1 18 Tf 100 500 Td (Across the page) Tj ET \
        BT /F1 12 Tf 0 1 -1 0 300 200 Tm (Up the page) Tj ET";
    let content = pdf.add_object(Stream::new(dictionary! {}, content.to_vec()));
    let page = pdf.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "CropBox" => vec![36.into(), 48.5.into(), 560.into(), 740.into()],
        "Rotate" => 90,
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        "Contents" => content,
    });
    pdf.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// Without the corpus: on a turned page whose crop box is off the origin,
/// each character's box shows within 1/16 pt of PDFium's, and its angle is
/// the way it's drawn, counter-clockwise.
#[test]
fn char_boxes_on_a_turned_page_off_the_origin_are_pdfium_s() {
    let bytes = turned_text();
    let src = sheaf_doc::forms::fixture::source(bytes.clone());
    assert!(moved(&src.geom(0)));
    let engine = PdfiumEngine::open_bytes(bytes.into(), None).unwrap();
    let (chars, far, differ) = char_boxes(&engine, &src, [0]);
    assert_eq!((chars, far), ("Across the pageUp the page".len(), 0));
    assert!(differ.is_empty(), "{differ:#?}");
    let read = engine.page_chars(PageIndex(0)).unwrap();
    let angles = |first: char| {
        let at = read.iter().position(|c| c.ch == first).unwrap();
        read[at..at + 3]
            .iter()
            .map(|c| c.angle.round())
            .collect::<Vec<f32>>()
    };
    assert_eq!(angles('A'), [0.0; 3]);
    assert_eq!(angles('U'), [90.0; 3]);
}
