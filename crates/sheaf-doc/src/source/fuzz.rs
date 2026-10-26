//! The bodies of the fuzz targets in `fuzz/` (cargo-fuzz), here so a
//! stable test runs them too. Each reads what it's given, and must
//! neither panic nor hang whatever the bytes. `op_json`'s document is
//! built from the form fixtures, and `interp`'s page from the content
//! fixtures, so they need the `fixtures` feature.

use std::borrow::Cow;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use lopdf::{LazyPdf, Object, Stream, dictionary};

use super::Limits;
use crate::PageId;
use crate::content::{
    self, Concat, DrawnForms, PageBlocks, PageContent, PropertiesDict, ResCx, SpanAcross,
};
use crate::private::LIST_KEYS;
use crate::store::{BlobKind, Src, Store};

/// `data` read lazily: every object the cross-reference table lists, and
/// where each is in the bytes.
pub fn lazy_reader(data: &[u8]) {
    let Ok(pdf) = LazyPdf::open(Arc::<[u8]>::from(data), None, &Limits::DEFAULT.reader) else {
        return;
    };
    let _ = pdf.catalog();
    for id in pdf.ids() {
        let _ = pdf.get(id);
        let _ = pdf.raw_span(id);
    }
}

/// `data` interned and opened as a source: its page table, every page's
/// size, and its conformance claims.
pub fn page_tree(data: &[u8]) {
    let kind = BlobKind::file(Path::new("fuzzed.pdf"), None);
    let Ok(src) = Store::new().open(data.into(), kind) else {
        return;
    };
    for i in 0..src.page_count() {
        let _ = src.page_size(i);
    }
    let _ = src.conformance();
}

/// `data` as the objects of a page's annotations, `n 0 obj … endobj` from
/// 4 up, read as the open reads sheaf's marks: each entry's `/SHEF_Annot`
/// and digest, and whether it's ours to rewrite. The page, turned 90°, lists an annotation for each
/// `endobj`, in a file with no cross-reference table, which the reader
/// rebuilds by scanning.
pub fn sheaf_annot(data: &[u8]) {
    let Some(src) = annotated_page(data) else {
        return;
    };
    let (pdf, Some(page)) = (src.pdf(), src.page(0)) else {
        return;
    };
    for entry in page.annots.iter() {
        let _ = crate::file::sheaf_mark(pdf, entry);
        let _ = crate::file::is_ours(pdf, entry);
        if let Ok((_, lopdf::Object::Dictionary(d))) = pdf.dereference(entry) {
            let _ = crate::annot::staleness(pdf, d, false);
        }
    }
}

/// `data` opened as a source, and the generators sheaf drew on it found as
/// an open finds them: from its catalog's `/SHEF_Generators` and the
/// markers of its pages' streams, its images and pages bound to the
/// XObjects their blocks draw, a header's Bates numbers its blocks'. Each
/// is made a generator of the pages it's on, and laid out as the realizer
/// lays it out to tell whether a page's block is it, on its first pages.
pub fn sheaf_generators(data: &[u8]) {
    let kind = BlobKind::file(Path::new("fuzzed.pdf"), None);
    let Ok(src) = Store::new().open(data.into(), kind) else {
        return;
    };
    for i in 0..src.page_count() {
        let _ = src.markers(i);
    }
    for saved in src.generators().iter() {
        let Some(g) = saved.generator(&src, |i| Some(PageId(u64::from(i)))) else {
            continue;
        };
        for i in 0..src.page_count().min(8) {
            let ctx = crate::model::PageCtx {
                geom: src.geom(i),
                position: i,
                count: src.page_count(),
                label: (i + 1).to_string().into(),
                bates: g.frozen.bates_number(PageId(u64::from(i))),
            };
            let _ = crate::materialize::emit::content::model_digest(&g, &ctx);
        }
    }
}

/// `data` as a page's content, lexed under the default caps, every token
/// taken: whole, and as two streams split at its middle. The resources
/// name a color space of each size an inline image may be drawn in.
pub fn lexer(data: &[u8]) {
    let res = ResCx::new([
        (b"CS1".to_vec(), 1),
        (b"CS3".to_vec(), 3),
        (b"CS4".to_vec(), 4),
    ]);
    let (first, second) = data.split_at(data.len() / 2);
    let split = Concat::new(&[first, second]);
    for content in [Concat::new(&[data]), split] {
        for token in content.lex(&res, &Limits::DEFAULT) {
            let Ok((range, _)) = token else { break };
            let _ = content.span(range);
        }
    }
}

/// `data` as a page's content, two streams split at its middle, drawn
/// with resources that name a property list, a color space, a form of
/// Acrobat's and one with an artifact in it: its marked ranges, names and
/// blocks, and a splice of each range at the top, of all of them at once
/// widened to touch ([`touching`]), and of its pagination artifacts as a
/// strip cuts them; and each range at the top with sheaf's private data
/// taken out of its list. Where a splice is made, what it
/// keeps lexes as the tokens it kept; where a list is written again, the
/// content lexes as it did but for the list, which reads as it did without
/// those keys. Returns how many were made.
pub fn splice(data: &[u8]) -> usize {
    let lim = &Limits::DEFAULT;
    let mut pdf = lopdf::Document::with_version("1.7");
    let (first, second) = data.split_at(data.len() / 2);
    let streams = [first, second].map(|s| pdf.add_object(Stream::new(dictionary! {}, s.to_vec())));
    let piece = dictionary! { "ADBE_CompoundType" => dictionary! { "Private" => "Header" } };
    let acrobat = pdf.add_object(Stream::new(
        dictionary! { "Subtype" => "Form", "PieceInfo" => piece },
        b"BT (h) Tj ET".to_vec(),
    ));
    let nested = pdf.add_object(Stream::new(
        dictionary! { "Subtype" => "Form" },
        b"/Artifact <</Subtype /Footer>> BDC EMC".to_vec(),
    ));
    let resources = Object::Dictionary(dictionary! {
        "Properties" => dictionary! { "P0" => dictionary! {
            "Type" => "Pagination", "Subtype" => "Header", "SHEF_Gen" => Object::string_literal("g1"),
        } },
        "XObject" => dictionary! { "X0" => acrobat, "X1" => nested },
        "ColorSpace" => dictionary! { "CS3" => "DeviceRGB" },
    });
    let Ok(content) = PageContent::read(&pdf, &streams, lim) else {
        return 0;
    };
    let concat = content.concat();
    let res = ResCx::read(&pdf, Some(&resources));
    let _ = content::balance(concat, &res, lim);
    let _ = content::names_used(concat, &res, lim, &[]);
    let props = PropertiesDict::read(&pdf, Some(&resources));
    let mut drops: Vec<Vec<SpanAcross>> = Vec::new();
    let mut top: Vec<SpanAcross> = Vec::new();
    if let Ok(found) = content::marked(concat, &props, &res, lim) {
        top = found
            .iter()
            .filter(|m| m.depth == 0)
            .map(|m| m.span)
            .collect();
        drops.extend(top.iter().map(|&span| vec![span]));
        if top.len() > 1 {
            drops.push(touching(concat, &top));
        }
    }
    let known = DrawnForms::default();
    if let Ok(blocks) = PageBlocks::of(&pdf, &content, Some(&resources), lim, &known)
        && let Ok(cuts) = blocks
            .pagination
            .iter()
            .map(|p| p.cuts(concat, &res, lim))
            .collect::<Result<Vec<_>, _>>()
        && !cuts.is_empty()
    {
        drops.push(cuts.concat());
    }
    let mut made = 0;
    for drop in drops {
        if let Ok(kept) = content::splice(concat, &res, lim, &drop) {
            assert_kept(concat, &res, &kept, &drop);
            made += 1;
        }
    }
    for span in top {
        if let Ok(Some(rewrite)) = content::without_keys(concat, &res, lim, span, &LIST_KEYS) {
            let nothing = content::splice_each(concat, &res, lim, &[]);
            let rewritten = (nothing.streams(concat, &[rewrite])).expect("a list written again");
            assert_rewritten(concat, &res, &rewritten);
            made += 1;
        }
    }
    made
}

/// `data` as a page's content, two streams split at its middle, drawn on
/// [`fuzz_page`](content::fixture::fuzz_page)'s page, whose resources
/// name a font of each kind, a color space of each size, a property list
/// with `/ActualText`, optional content, and forms that draw themselves,
/// a chain of two and a form in a form: interpreted, its forms drawn,
/// each glyph laid out with its box. Where `data` has [`FONT_PART`]s,
/// the content is what's before the first, and what's after each is,
/// in turn, `/CM`'s embedded CMap, `/T3`'s `a` CharProc, `/I2`'s `/W`
/// and `/V`'s `/W2`, `/T3`'s `/FontMatrix`, and `/TT`'s program, which
/// the CIDFonts share ([`with_fonts`]), so the fonts' readers read what
/// the fuzzer makes too, a program's reader among them. Where that reads, it's
/// written again ([`encode`](content::interp::encode)) and the encoding
/// drawn, which draws the same, but operators: as many events and
/// glyphs, each where it was, to the bit. Returns how many events it
/// drew.
#[cfg(any(test, feature = "fixtures"))]
pub fn interp(data: &[u8]) -> usize {
    let parts = font_parts(data);
    match parts[..] {
        [content] => INTERP.with(|(page, fonts)| interp_on(page, fonts, content)),
        [content, ref fonts @ ..] => INTERP.with(|(page, _)| {
            let page = with_fonts(page, fonts);
            interp_on(&page, &content::Fonts::default(), content)
        }),
        [] => 0,
    }
}

/// What splits [`interp`]'s input into its content and the fonts' parts.
#[cfg(any(test, feature = "fixtures"))]
pub const FONT_PART: &[u8] = b"\n%%font\n";

/// `data` split at each [`FONT_PART`], into six parts at most.
#[cfg(any(test, feature = "fixtures"))]
fn font_parts(data: &[u8]) -> Vec<&[u8]> {
    let mut parts = Vec::new();
    let mut rest = data;
    while parts.len() < 5
        && let Some(at) = rest.windows(FONT_PART.len()).position(|w| w == FONT_PART)
    {
        parts.push(&rest[..at]);
        rest = &rest[at + FONT_PART.len()..];
    }
    parts.push(rest);
    parts
}

/// `page`, the fuzz page, with what `parts` give its fonts, each where
/// it's there: the bytes of `/CM`'s embedded CMap and of `/T3`'s `a`
/// CharProc, objects, where they read as arrays, for `/I2`'s `/W` and
/// `/V`'s `/W2`, and `/T3`'s `/FontMatrix`, and the bytes of `/TT`'s
/// `/FontFile2`, the program the CIDFonts embed too.
#[cfg(any(test, feature = "fixtures"))]
fn with_fonts(page: &content::fixture::Fixture, parts: &[&[u8]]) -> content::fixture::Fixture {
    let mut pdf = page.pdf.clone();
    let font = |name: &[u8]| -> lopdf::ObjectId {
        let fonts = page
            .resources()
            .and_then(|r| r.as_dict().ok()?.get(b"Font").ok());
        let font = fonts.and_then(|f| f.as_dict().ok()?.get(name).ok()?.as_reference().ok());
        font.expect("the fuzz page has the font")
    };
    let get = |pdf: &lopdf::Document, id, key: &[u8]| -> Option<Object> {
        Some(pdf.get_dictionary(id).ok()?.get(key).ok()?.clone())
    };
    let descendant = |pdf: &lopdf::Document, name| match get(pdf, font(name), b"DescendantFonts") {
        Some(Object::Array(fonts)) => fonts.first().and_then(|f| f.as_reference().ok()),
        _ => None,
    };
    let array = |part: &[u8]| match content::object(part, Limits::DEFAULT.content.depth) {
        Some(array @ Object::Array(_)) => Some(array),
        _ => None,
    };
    let mut set_stream = |id: Option<lopdf::ObjectId>, data: &[u8]| {
        if let Some(stream) = id.and_then(|id| pdf.get_object_mut(id).ok()?.as_stream_mut().ok()) {
            stream.set_plain_content(data.to_vec());
        }
    };
    let cmap = get(&page.pdf, font(b"CM"), b"Encoding").and_then(|e| e.as_reference().ok());
    let procs = get(&page.pdf, font(content::fixture::SQUARES), b"CharProcs");
    let proc = procs.and_then(|p| p.as_dict().ok()?.get(b"a").ok()?.as_reference().ok());
    let descriptor = get(&page.pdf, font(b"TT"), b"FontDescriptor");
    let descriptor = descriptor.and_then(|d| d.as_reference().ok());
    let program = descriptor.and_then(|d| get(&page.pdf, d, b"FontFile2")?.as_reference().ok());
    for (id, data) in [cmap, proc].into_iter().zip(parts) {
        set_stream(id, data);
    }
    if let Some(data) = parts.get(4) {
        set_stream(program, data);
    }
    if let Some(widths) = parts.get(2).copied().and_then(array) {
        let cids = [
            (descendant(&pdf, b"I2"), &b"W"[..]),
            (descendant(&pdf, b"V"), b"W2"),
        ];
        for (id, key) in cids {
            if let Some(cid_font) = id.and_then(|id| pdf.get_dictionary_mut(id).ok()) {
                cid_font.set(key, widths.clone());
            }
        }
    }
    if let Some(matrix) = parts.get(3).copied().and_then(array)
        && let Ok(t3) = pdf.get_dictionary_mut(font(content::fixture::SQUARES))
    {
        t3.set("FontMatrix", matrix);
    }
    content::fixture::Fixture { pdf }
}

/// [`interp`]'s body: `data` drawn on `page`, with `fonts`.
#[cfg(any(test, feature = "fixtures"))]
fn interp_on(page: &content::fixture::Fixture, fonts: &content::Fonts, data: &[u8]) -> usize {
    use content::interp::{EncodeError, encode};
    let (first, second) = data.split_at(data.len() / 2);
    let content = Concat::new(&[first, second]);
    let Ok(drawn) = drawn_on(page, &content, fonts) else {
        return 0;
    };
    let lim = &Limits::DEFAULT;
    let encoded = match encode(&page.pdf, &content, page.resources(), lim, fonts) {
        Ok(encoded) => encoded,
        // What can't be written to read back the same isn't.
        Err(EncodeError::Unwritable { .. }) => return drawn.events,
        Err(e) => panic!("it's drawn, but isn't written again: {e}"),
    };
    let again = page.encoded(&encoded);
    let redrawn = drawn_on(&again, &Concat::new(&[&encoded.content]), fonts);
    let redrawn = redrawn.expect("the content written again is drawn");
    assert_eq!(drawn, redrawn, "written again, it draws otherwise");
    drawn.events
}

#[cfg(any(test, feature = "fixtures"))]
thread_local! {
    static INTERP: (content::fixture::Fixture, content::Fonts) =
        (content::fixture::fuzz_page(), content::Fonts::default());
}

/// What content drew, but its operators.
#[cfg(any(test, feature = "fixtures"))]
#[derive(Debug, PartialEq, Eq)]
struct Drawn {
    events: usize,
    glyphs: usize,
    /// Where each thing drawn is, to the bit but for a 0's sign: each
    /// glyph's code, origin, box and advance, each path's segments and
    /// bounds, each image's corners, each shading's bounds, each form's
    /// matrix and box, and each marked range's tag and MCID, hashed in
    /// order.
    placed: u64,
}

/// What `content`, drawn on `page`, draws, each glyph laid out with its
/// box ([`Drawn`]).
#[cfg(any(test, feature = "fixtures"))]
fn drawn_on(
    page: &content::fixture::Fixture,
    content: &Concat,
    fonts: &content::Fonts,
) -> Result<Drawn, content::interp::InterpError> {
    use std::hash::{DefaultHasher, Hash, Hasher};

    use content::interp::{Kind, Opts, Seg, interpret};
    let (mut events, mut glyphs) = (0, 0);
    let mut placed = DefaultHasher::new();
    // Each number's bits, a 0 of either sign the same.
    let mut put = |numbers: &[f32]| {
        for v in numbers {
            (v + 0.0).to_bits().hash(&mut placed);
        }
    };
    let rect = |r: crate::geom::Rect| [r.x0, r.y0, r.x1, r.y1];
    let lim = &Limits::DEFAULT;
    interpret(
        &page.pdf,
        content,
        page.resources(),
        lim,
        fonts,
        Opts::default(),
        |event| {
            if !matches!(event.kind, Kind::Op) {
                events += 1;
            }
            match event.kind {
                Kind::Op => {}
                Kind::Text(show) => {
                    for g in show.glyphs() {
                        glyphs += 1;
                        put(&[f32::from_bits(g.code), g.advance, g.origin.x, g.origin.y]);
                        for p in g.quad.0 {
                            put(&[p.x, p.y]);
                        }
                    }
                }
                Kind::Path(path) => {
                    put(&rect(path.bounds));
                    for seg in path.segments {
                        match *seg {
                            Seg::Move(p) | Seg::Line(p) => put(&[p.x, p.y]),
                            Seg::Curve(a, b, p) => put(&[a.x, a.y, b.x, b.y, p.x, p.y]),
                            Seg::Close => put(&[f32::NAN]),
                        }
                    }
                }
                Kind::Image(image) => {
                    for p in image.quad.0 {
                        put(&[p.x, p.y]);
                    }
                }
                Kind::Shading(shading) => put(&shading.bounds.map_or([f32::NAN; 4], rect)),
                Kind::FormBegin(form) => {
                    let m = form.matrix;
                    put(&[m.a, m.b, m.c, m.d, m.e, m.f]);
                    put(&form.bbox.map_or([f32::NAN; 4], rect));
                }
                Kind::MarkedBegin(mark) | Kind::MarkedPoint(mark) => {
                    let mcid = mark.mcid.unwrap_or(u32::MAX);
                    put(&[f32::from_bits(mcid), f32::from_bits(mark.tag.len() as u32)]);
                }
                _ => {}
            }
            std::ops::ControlFlow::Continue(())
        },
    )?;
    Ok(Drawn {
        events,
        glyphs,
        placed: placed.finish(),
    })
}

/// `data` as Op JSON (M10.5 §6.1): an envelope that reads is applied by
/// the user to a small document ([`OpJson`]): its ops in one batch, or
/// one by one when the batch fails, the files the fixture's ops name
/// granted, then committed, undone and redone. Whatever the ops say, each
/// applies or fails with an error, and what's committed keeps the state's
/// rules ([`DocState::check_invariants`]), undone and redone too. Returns
/// how many ops applied.
///
/// [`DocState::check_invariants`]: crate::model::DocState::check_invariants
#[cfg(any(test, feature = "fixtures"))]
pub fn op_json(data: &[u8]) -> usize {
    let Ok(envelope) = serde_json::from_slice::<sheaf_edit::Envelope>(data) else {
        return 0;
    };
    OP_JSON.with(|doc| doc.apply(envelope.ops))
}

#[cfg(any(test, feature = "fixtures"))]
thread_local! {
    static OP_JSON: OpJson = OpJson::new();
}

/// [`op_json`]'s document: four pages, a form with every kind of field on
/// the first two, sheaf's marks of every kind on the third, and on the
/// fourth another tool's comment with a reply; bookmarks, roman labels on
/// the first two pages and a header Acrobat drew on the last two.
/// The names the Op JSON fixture's references use are there
/// (`Part I › Ch. 2`, `Appendix`, `a1b2-c3`, `acro-1`, `sheaf:12`). Read
/// once, opened afresh each time, with the files its ops are granted: a
/// stamp's page, an attachment's file and a file of two pages as assets 0,
/// 1 and 2, and one of a page as `logo`.
#[cfg(any(test, feature = "fixtures"))]
struct OpJson {
    store: Arc<Store>,
    primary: Src,
    assets: Vec<crate::store::Blob>,
    logo: crate::store::Blob,
}

#[cfg(any(test, feature = "fixtures"))]
impl OpJson {
    fn new() -> OpJson {
        use crate::forms::fixture::{
            Form, MARK_FILE_BYTES, every_kind_in, every_mark_kind, holding, mark_stamp_pdf,
            outline_tree, write_marks,
        };
        let mut f = every_kind_in(4);
        let pages = f.pages.clone();
        let mut third = f.dict(pages[2]).clone();
        write_marks(&mut f.pdf, &mut third, &holding(&every_mark_kind()));
        *f.dict(pages[2]) = third;
        let rect = || Object::Array([72, 600, 92, 620].map(Object::from).to_vec());
        let comment = f.pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(),
            "NM" => Object::string_literal("a1b2-c3"), "Contents" => Object::string_literal("Why?"),
        });
        let reply = f.pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(), "IRT" => comment,
            "NM" => Object::string_literal("acro-1"), "Contents" => Object::string_literal("Because."),
        });
        f.set_annots(3, &[comment, reply]);
        let item = |title: &str, page| {
            dictionary! {
                "Title" => Object::string_literal(title),
                "Dest" => vec![Object::Reference(page), "Fit".into()],
            }
        };
        outline_tree(
            &mut f,
            vec![
                (0, item("Part I", pages[0])),
                (1, item("Ch. 1", pages[0])),
                (1, item("Ch. 2", pages[1])),
                (0, item("Appendix", pages[3])),
            ],
        );
        let catalog = f.catalog;
        let labels = vec![
            0.into(),
            dictionary! { "S" => "r" }.into(),
            2.into(),
            dictionary! { "S" => "D" }.into(),
        ];
        f.dict(catalog)
            .set("PageLabels", dictionary! { "Nums" => labels });
        for &page in &pages[2..] {
            let mut dict = f.dict(page).clone();
            crate::generate::fixture::acrobat_header(&mut f.pdf, &mut dict);
            *f.dict(page) = dict;
        }
        let store = Store::new();
        let kind = BlobKind::file(Path::new("form.pdf"), None);
        let primary = store
            .open(f.bytes().into(), kind)
            .expect("the fixture opens");
        let intern = |bytes: Vec<u8>, kind| store.intern(bytes.into(), kind).expect("a blob");
        let scan = BlobKind::file(Path::new("scan.pdf"), None);
        let assets = vec![
            intern(mark_stamp_pdf(), BlobKind::stamp()),
            intern(MARK_FILE_BYTES.to_vec(), BlobKind::Attachment),
            intern(Form::new(2).bytes(), scan),
        ];
        let logo = intern(
            mark_stamp_pdf(),
            BlobKind::file(Path::new("logo.pdf"), None),
        );
        OpJson {
            store,
            primary,
            assets,
            logo,
        }
    }

    /// `ops` applied, as [`op_json`] says: how many applied.
    fn apply(&self, ops: Vec<sheaf_edit::Op>) -> usize {
        use crate::tx::{Actor, CommitOpts, Grant};
        use in_progress::NoProgress;
        let policy = Box::new(crate::history::FixedCount(4));
        let (store, name) = (self.store.clone(), "form.pdf".into());
        let mut d = crate::open::start(store, self.primary.clone(), name, None, policy);
        d.ready(crate::open::finish(&self.primary, &NoProgress))
            .expect("the fixture's facts");
        let mut tx = d.tx(Actor::User);
        for blob in &self.assets {
            tx.grant(Grant::Blob(blob.clone().into()));
        }
        tx.grant_named("logo", Grant::Blob(self.logo.clone().into()));
        let applied = match tx.apply_batch(ops.clone(), &NoProgress) {
            Ok(done) => done.len(),
            Err(_) => (ops.into_iter())
                .filter(|op| tx.apply(op.clone(), &NoProgress).is_ok())
                .count(),
        };
        let Ok(commit) = d.commit(tx, CommitOpts::default()) else {
            return applied;
        };
        let kept = |d: &crate::model::Document| assert_eq!(d.state().check_invariants(), Ok(()));
        kept(&d);
        if commit.version.is_some() {
            d.undo().expect("its version undone");
            kept(&d);
            d.redo().expect("and redone");
            kept(&d);
        }
        applied
    }
}

/// `spans`, in order and apart, each widened over the white-space either
/// side of it, white-space between two split at its middle: cuts that
/// touch, so whether one needs a space in its place depends on bytes its
/// neighbour takes.
fn touching(content: &Concat, spans: &[SpanAcross]) -> Vec<SpanAcross> {
    let s = content.bytes();
    let space = |c: &&u8| matches!(c, b'\0' | b'\t' | b'\n' | b'\x0c' | b'\r' | b' ');
    let ranges: Vec<Range<usize>> = spans
        .iter()
        .filter_map(|&span| content.offsets(span))
        .collect();
    let mut cuts: Vec<Range<usize>> = ranges
        .iter()
        .map(|r| {
            let before = s[..r.start].iter().rev().take_while(space).count();
            r.start - before..r.end + s[r.end..].iter().take_while(space).count()
        })
        .collect();
    for i in 1..cuts.len() {
        if cuts[i].start < cuts[i - 1].end {
            let middle = (ranges[i - 1].end + ranges[i].start) / 2;
            (cuts[i - 1].end, cuts[i].start) = (middle, middle);
        }
    }
    cuts.into_iter().map(|r| content.span(r)).collect()
}

/// That `kept`, `content` spliced without `drop`, lexes as the tokens of
/// `content` outside `drop`.
fn assert_kept(content: &Concat, res: &ResCx, kept: &[Cow<[u8]>], drop: &[SpanAcross]) {
    let lim = &Limits::DEFAULT;
    let drop: Vec<_> = drop.iter().filter_map(|&d| content.offsets(d)).collect();
    let before: Vec<&[u8]> = content
        .lex(res, lim)
        .map(|t| t.expect("a splice lexed it"))
        .filter(|(r, _)| !drop.iter().any(|d| d.start <= r.start && r.end <= d.end))
        .map(|(r, _)| &content.bytes()[r])
        .collect();
    let kept: Vec<&[u8]> = kept.iter().map(|s| &**s).collect();
    let kept = Concat::new(&kept);
    let after: Vec<&[u8]> = kept
        .lex(res, lim)
        .map(|t| t.expect("what a splice keeps lexes"))
        .map(|(r, _)| &kept.bytes()[r])
        .collect();
    assert_eq!(before, after, "the tokens a splice kept");
}

/// That `rewritten`, `content` with a list written again without
/// [`LIST_KEYS`], lexes as `content` does but for that list, which reads
/// as it did without them.
fn assert_rewritten(content: &Concat, res: &ResCx, rewritten: &[Cow<[u8]>]) {
    let lim = &Limits::DEFAULT;
    let tokens = |c: &Concat| -> Vec<Vec<u8>> {
        (c.lex(res, lim))
            .map(|t| c.bytes()[t.expect("a list written again lexes").0].to_vec())
            .collect()
    };
    let streams: Vec<&[u8]> = rewritten.iter().map(|s| &**s).collect();
    let (before, after) = (tokens(content), tokens(&Concat::new(&streams)));
    assert_eq!(
        before.len(),
        after.len(),
        "the tokens a list written again kept"
    );
    let changed: Vec<_> = before.iter().zip(&after).filter(|(b, a)| b != a).collect();
    let [(was, now)] = changed[..] else {
        panic!("a list written again is one token: {changed:?}")
    };
    let read = |list: &[u8]| content::object(list, lim.content.depth);
    let Some(Object::Dictionary(mut left)) = read(was) else {
        panic!("a list written again was one")
    };
    for key in LIST_KEYS {
        left.remove(key);
    }
    assert_eq!(
        read(now),
        Some(Object::Dictionary(left)),
        "the list written again"
    );
}

/// [`sheaf_annot`]'s file, opened as a source.
fn annotated_page(data: &[u8]) -> Option<Src> {
    let n = data.windows(6).filter(|w| w == b"endobj").count();
    let annots: String = (0..n).map(|i| format!("{} 0 R ", i + 4)).collect();
    let mut file = format!(
        "%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
         2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
         3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [-37 52 575 844] /Rotate 90 \
         /Annots [{annots}] >>\nendobj\n"
    )
    .into_bytes();
    file.extend_from_slice(data);
    file.extend_from_slice(b"\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n");
    let kind = BlobKind::file(Path::new("fuzzed.pdf"), None);
    Store::new().open(file.into(), kind).ok()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::time::{Duration, Instant};

    use lopdf::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter};
    use lopdf::xref::XrefType;
    use lopdf::{EncryptionState, EncryptionVersion, Permissions};

    use lopdf::{Document as Pdf, Object, ObjectId};

    use super::*;
    use crate::forms::fixture::{Form, every_kind, every_mark_kind, hybrid_reference};
    use crate::serialize::fixture::{encrypted, linearized_file};
    use crate::source::lazy_tests::{plaintext, stream, with_table};

    /// Mutations of each fixture.
    const CASES: usize = 600;

    /// Mutations of each of [`interp`]'s pages, which it draws twice and
    /// writes once.
    const INTERP_CASES: usize = 200;

    /// What a mutation may write: the tokens files are made of, and
    /// numbers that are offsets, lengths and counts.
    const TOKENS: &[&[u8]] = &[
        b" 0 R ",
        b" 0 obj ",
        b"endobj",
        b"<<",
        b">>",
        b"[",
        b"]",
        b"(",
        b")",
        b"/Kids [2 0 R] ",
        b"/Parent 3 0 R ",
        b"/Type /Pages ",
        b"/Count 1048576 ",
        b"/MediaBox [0 0 1e30 -7] ",
        b"/Rotate -2147483648 ",
        b"/Length 4 0 R ",
        b"/Type /ObjStm /N 9 /First 0 ",
        b"/Filter /FlateDecode ",
        b"stream\n",
        b"endstream",
        b"xref\n0 3\n",
        b"trailer",
        b"startxref\n0\n",
        b"%%EOF",
        b"4294967296",
        b"-1",
    ];

    /// What a mutation of content may write: its tokens, the bytes that
    /// end an inline image's data, and what moves text and draws forms.
    const CONTENT_TOKENS: &[&[u8]] = &[
        b" ",
        b"(",
        b")",
        b"\\",
        b"<",
        b">",
        b"<<",
        b">>",
        b"[",
        b"]",
        b"%",
        b"\n",
        b" BI ",
        b" ID ",
        b" EI ",
        b"/W 4 /H 1 /BPC 8 /CS /CS3 ",
        b"/F /DCT ",
        b"/L 9 ",
        b"~>",
        b"\xff\xd9",
        b"\x80",
        b" BDC ",
        b" EMC ",
        b" q ",
        b" Q ",
        b" BT ",
        b" ET ",
        b" /F1 9 Tf ",
        b" 0 -14 Td ",
        b" (x) Tj ",
        b" /X0 Do ",
        b" /Artifact /P0 BDC ",
        b" /Artifact <</Type /Pagination /Subtype /Footer>> BDC ",
        b"4294967296",
        b" 12 TL ",
        b" 5 -14 TD ",
        b" T* ",
        b" (a) ' ",
        b" 2 1 (a b) \" ",
        b" 0 1 -1 0 300 500 Tm ",
        b" /T3 10 Tf ",
        b" /X1 Do ",
        b" /Fm1 Do ",
        b" /OC /OC0 BDC ",
    ];

    /// Mutations of each Op JSON fixture: fewer, as each opens a document
    /// and commits; ten times as many of the ops that apply, which reach
    /// further.
    const OP_CASES: usize = 100;

    /// What a mutation of Op JSON may write: its punctuation and values,
    /// references in their forms, and numbers past what ids, positions and
    /// floats hold.
    const JSON_TOKENS: &[&[u8]] = &[
        b"\"",
        b"{",
        b"}",
        b"[",
        b"]",
        b",",
        b":",
        b"null",
        b"true",
        b"-1",
        b"0",
        b"1e309",
        b"-0.0",
        b"4294967296",
        b"9007199254740993",
        b"\"$1\"",
        b"\"$0\"",
        b"{\"pos\":0}",
        b"{\"pos\":4}",
        b"\"all\"",
        b"{\"nm\":\"sheaf:12\"}",
        b"{\"path\":[]}",
        b"{\"asset\":9}",
        b"\"1-4,2\"",
        b"\"\\u0000\"",
    ];

    fn mutate(rng: &mut fastrand::Rng, data: &mut Vec<u8>, tokens: &[&[u8]]) {
        if data.is_empty() {
            data.extend_from_slice(tokens[rng.usize(..tokens.len())]);
            return;
        }
        let at = rng.usize(..data.len());
        match rng.u8(..5) {
            0 => data[at] = rng.u8(..),
            1 => {
                let end = (at + rng.usize(1..=64)).min(data.len());
                data.drain(at..end);
            }
            2 => {
                let token = tokens[rng.usize(..tokens.len())];
                data.splice(at..at, token.iter().copied());
            }
            3 => {
                let end = (at + rng.usize(1..=256)).min(data.len());
                let copy = data[at..end].to_vec();
                let to = rng.usize(..=data.len());
                data.splice(to..to, copy);
            }
            _ => {
                // A number written over a number.
                let end = at + data[at..].iter().take_while(|c| c.is_ascii_digit()).count();
                let number = rng.u64(..).to_string();
                let number = &number[..rng.usize(1..=number.len())];
                data.splice(at..end, number.bytes());
            }
        }
    }

    /// A change to `json`, an envelope, that keeps it JSON, so its ops
    /// mostly still read and are lowered: one of its ops dropped, copied
    /// elsewhere or swapped with another; or anywhere in it, a number
    /// replaced by another of its kind, a string by another or by one
    /// that names something else or nothing, an element of a list dropped
    /// or copied, a field dropped or made null.
    fn mutate_json(rng: &mut fastrand::Rng, json: &mut serde_json::Value) {
        use serde_json::{Value, json};
        fn count(v: &Value) -> usize {
            1 + match v {
                Value::Array(a) => a.iter().map(count).sum(),
                Value::Object(o) => o.values().map(count).sum(),
                _ => 0,
            }
        }
        /// The `k`th node of `v`, in pre-order.
        fn nth<'a>(v: &'a mut Value, k: &mut usize) -> Option<&'a mut Value> {
            if *k == 0 {
                return Some(v);
            }
            *k -= 1;
            match v {
                Value::Array(a) => a.iter_mut().find_map(|x| nth(x, k)),
                Value::Object(o) => o.values_mut().find_map(|x| nth(x, k)),
                _ => None,
            }
        }
        if let Some(ops) = json["ops"].as_array_mut().filter(|o| o.len() > 1)
            && rng.bool()
        {
            let (i, j) = (rng.usize(..ops.len()), rng.usize(..ops.len()));
            match rng.u8(..3) {
                0 => drop(ops.remove(i)),
                1 => ops.insert(j, ops[i].clone()),
                _ => ops.swap(i, j),
            }
            return;
        }
        let mut k = rng.usize(..count(json));
        let node = nth(json, &mut k).expect("a node of it");
        match node {
            Value::Number(n) if n.is_f64() => {
                let floats = [0.0, -1.0, 0.5, 612.0, 1e30];
                *node = json!(floats[rng.usize(..floats.len())]);
            }
            Value::Number(_) => {
                let ints = [0, 1, 2, 3, 4, 7, 12, 99, 4294967295_u64];
                *node = json!(ints[rng.usize(..ints.len())]);
            }
            Value::String(_) => {
                let strings = ["$1", "$5", "$9", "all", "sheaf:12", "a1b2-c3", "", "1-9"];
                *node = json!(strings[rng.usize(..strings.len())]);
            }
            Value::Array(a) if !a.is_empty() => {
                let i = rng.usize(..a.len());
                if rng.bool() {
                    a.remove(i);
                } else {
                    let copy = a[i].clone();
                    a.insert(rng.usize(..=a.len()), copy);
                }
            }
            Value::Object(o) if !o.is_empty() => {
                let key = o.keys().nth(rng.usize(..o.len())).cloned().expect("a key");
                match rng.bool() {
                    true => o.remove(&key),
                    false => o.insert(key, Value::Null),
                };
            }
            _ => *node = Value::Null,
        }
    }

    const XMP: &[u8] = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" \
        xmlns:a=\"http://www.aiim.org/pdfa/ns/id/\" a:part=\"1\"/>";

    fn fixtures() -> Vec<Vec<u8>> {
        let tree = with_table(&[
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /Metadata 7 0 R >>".to_vec(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 3 /MediaBox [0 0 600 800] >>".to_vec(),
            ),
            (
                3,
                b"<< /Type /Pages /Parent 2 0 R /Kids [5 0 R 5 0 R] /Rotate 90 >>".to_vec(),
            ),
            (
                4,
                b"<< /Type /Page /Parent 2 0 R /CropBox [10 10 500 700] /Annots [6 0 R] >>"
                    .to_vec(),
            ),
            (
                5,
                b"<< /Type /Page /Parent 3 0 R /Contents 6 0 R >>".to_vec(),
            ),
            (6, stream("/Length 8", b"0 0 m S\n")),
            (
                7,
                stream(&format!("/Type /Metadata /Length {}", XMP.len()), XMP),
            ),
        ]);
        let mut modern = every_kind();
        let mut streams = Vec::new();
        modern.pdf.save_modern(&mut streams).unwrap();
        let mut classic = every_kind();
        classic.pdf.reference_table.cross_reference_type = XrefType::CrossReferenceTable;
        let std_cf = BTreeMap::from([(
            b"StdCF".to_vec(),
            std::sync::Arc::new(Aes128CryptFilter) as std::sync::Arc<dyn CryptFilter>,
        )]);
        let plain = plaintext();
        // The empty user password: the bodies open it, and decrypt.
        let aes = EncryptionState::try_from(EncryptionVersion::V4 {
            document: &plain,
            encrypt_metadata: true,
            crypt_filters: std_cf,
            stream_filter: b"StdCF".to_vec(),
            string_filter: b"StdCF".to_vec(),
            owner_password: "owner",
            user_password: "",
            permissions: Permissions::all(),
        })
        .unwrap();
        let [aes_table, aes_stream] = encrypted(&plain, &aes, |_| {});
        vec![
            tree,
            classic.bytes(),
            streams,
            hybrid_reference(),
            aes_table,
            aes_stream,
            linearized_file(),
        ]
    }

    /// [`sheaf_generators`]' fixtures: files sheaf saved with a watermark of
    /// each kind on every page and a background on some, text over an image,
    /// and a PDF's page over text; and a header and footer, of page numbers
    /// and Bates numbers on every page, then of labels on some.
    fn generator_fixtures() -> Vec<Vec<u8>> {
        use sheaf_edit::{BlobSel, GridPos, WatermarkContent, WatermarkSpec, Z};
        let image = crate::materialize::watermark_tests::png();
        let mut d = crate::tx::tests::doc_of(3, &[]);
        let second = d.page(1).unwrap();
        let png = (d.store())
            .intern(image.clone().into(), BlobKind::image(&image).unwrap())
            .unwrap();
        let art = |content| WatermarkSpec {
            content,
            position: GridPos::BottomRight,
        };
        let image = art(WatermarkContent::Image {
            image: Some(BlobSel::Id(png.id())),
            scale: 0.25,
            opacity: 0.5,
        });
        let page = art(WatermarkContent::Page {
            pdf: Some(BlobSel::Id(d.primary().id())),
            page: 1,
            scale: 0.5,
            opacity: 0.5,
        });
        let mut numbered = crate::tx::fixture::header("Page <<1>> of <<N>>");
        numbered.texts[5] = "<<bates>>".into();
        numbered.bates = Some(sheaf_edit::BatesSpec::default());
        let numbered = crate::tx::fixture::set_header(numbered, None);
        let labeled = crate::tx::fixture::header("<<label>> <<date>>");
        let labeled = crate::tx::fixture::set_header(labeled, Some(&[second]));
        let mut out = Vec::new();
        for (over, under, header) in [
            (crate::tx::fixture::watermark("DRAFT"), image, numbered),
            (page, crate::tx::fixture::watermark("under"), labeled),
        ] {
            let ops = vec![
                crate::tx::fixture::set_watermark(over, Z::Over, None),
                crate::tx::fixture::set_watermark(under, Z::Under, Some(&[second])),
                header,
            ];
            d.commit_ops(ops, "watermark").unwrap();
            out.push(crate::materialize::How::Rewritten.write(&d));
        }
        out
    }

    /// Unmutated, the generators' fixtures are found again, all three of
    /// each, a header's Bates numbers too.
    #[test]
    fn the_generator_fixtures_are_found() {
        for fixture in generator_fixtures() {
            let kind = BlobKind::file(Path::new("fixture.pdf"), None);
            let src = Store::new().open(fixture.as_slice().into(), kind).unwrap();
            let found = src.generators();
            let pages: Vec<Option<Vec<u32>>> = found.iter().map(|g| g.pages.clone()).collect();
            assert_eq!(found.len(), 3, "{pages:?}");
            assert!(pages.contains(&None) && pages.contains(&Some(vec![1])));
            let page = |i: u32| Some(PageId(u64::from(i)));
            let headers: Vec<_> = (found.iter())
                .filter_map(|s| s.generator(&src, page))
                .filter(|g| g.spec.header_footer().is_some())
                .collect();
            let [header] = &headers[..] else {
                panic!("one header: {pages:?}")
            };
            let numbers = header.frozen.bates.as_deref().map(|b| b.len());
            assert!(matches!(numbers, Some(3) | None), "{numbers:?}");
            for saved in found.iter() {
                assert!(
                    saved
                        .generator(&src, |i| Some(PageId(u64::from(i))))
                        .is_some()
                );
            }
        }
    }

    /// [`sheaf_annot`]'s fixtures: every kind of mark as a save writes it,
    /// the annotations' objects as lopdf writes them,
    /// numbered from 4 in `/Annots` order, without the appearances and
    /// files they refer to, but for what makes a page stamp and an
    /// attachment marks: the stamp's appearance, after them, which names
    /// its page, and the attachment's file specification, which names its
    /// file. Neither the page nor the file is in it.
    fn mark_fixtures() -> Vec<Vec<u8>> {
        let mut f = Form::new(1);
        let mut page = f.dict(f.pages[0]).clone();
        let marks = crate::forms::fixture::holding(&every_mark_kind());
        crate::forms::fixture::write_marks(&mut f.pdf, &mut page, &marks);
        let entries: Vec<ObjectId> = page
            .get(b"Annots")
            .and_then(Object::as_array)
            .unwrap()
            .iter()
            .map(|e| e.as_reference().unwrap())
            .collect();
        let number: HashMap<ObjectId, ObjectId> =
            (4..).zip(&entries).map(|(n, id)| (*id, (n, 0))).collect();
        // The appearance that draws a form: the page stamp's.
        let draws_a_form = |d: &lopdf::Dictionary| {
            let n = d
                .get(b"AP")
                .and_then(Object::as_dict)
                .ok()?
                .get(b"N")
                .ok()?;
            let n = n.as_reference().ok()?;
            let s = f.pdf.get_object(n).ok()?.as_stream().ok()?;
            let resources = s.dict.get(b"Resources").and_then(Object::as_dict).ok()?;
            resources.has(b"XObject").then(|| s.clone())
        };
        let mut out = Pdf::with_version("1.7");
        out.reference_table.cross_reference_type = XrefType::CrossReferenceTable;
        let mut next = 4 + entries.len() as u32;
        for id in &entries {
            let mut d = f.pdf.get_dictionary(*id).unwrap().clone();
            let ap = draws_a_form(&d);
            for key in [b"AP".as_slice(), b"P"] {
                d.remove(key);
            }
            if let Some(stream) = ap {
                out.objects.insert((next, 0), stream.into());
                d.set("AP", dictionary! { "N" => (next, 0) });
                next += 1;
            }
            if let Ok(irt) = d.get(b"IRT").and_then(Object::as_reference) {
                d.set("IRT", number[&irt]);
            }
            out.objects.insert(number[id], d.into());
        }
        let mut bytes = Vec::new();
        out.save_to(&mut bytes).unwrap();
        let at = |token: &[u8]| bytes.windows(token.len()).position(|w| w == token).unwrap();
        vec![bytes[at(b"4 0 obj")..at(b"xref")].to_vec()]
    }

    /// [`lexer`]'s fixtures: page content with an inline image found by
    /// each of PDFium's rules, ` EI ` in its data where the data is
    /// counted, and strings, arrays, dictionaries and marked content.
    fn content_fixtures() -> Vec<Vec<u8>> {
        let image = |dict: &str, data: &[u8]| {
            [format!("q BI {dict} ID ").as_bytes(), data, b"\nEI Q\n"].concat()
        };
        vec![
            image("/W 4 /H 1 /BPC 8 /CS /G", b" EI "),
            image("/W 2 /H 1 /BPC 8 /CS /CS3", b"a EI b"),
            image("/IM true /W 10 /H 2", b" EI "),
            image("/W 2 /H 2 /BPC 8 /CS [/I /RGB 1 <000000FFFFFF>]", b"EI E"),
            image("/F /AHx /W 2 /H 2 /BPC 8 /CS /G", b"0E EI 0>"),
            image("/F [/A85 /Fl] /W 9 /H 9", b"9jqo^ EI ~>"),
            image("/F /RL /W 4 /H 1 /BPC 8 /CS /G", b"\x03 EI \x80"),
            image("/F /DCT /W 9 /H 9 /BPC 8 /CS /RGB", b"\xff\xd8 EI \xff\xd9"),
            image("/F /Fl /L 6", b"x EI y"),
            image("/F /CCF /DP <</K -1>>", b"xEI EIy"),
            b"%c\nq /Artifact <</Type /Pagination /Subtype /Header /Attached [/Top]>> BDC\n\
              q BT /F1 12 Tf 72 720 Td [(a\\)(b) c) -250 <4142>] TJ (\\\\) Tj ET Q EMC\n\
              [1 [2 [3]] <</A <</B (c)>>>>] true false null Q { } ) ]"
                .to_vec(),
        ]
    }

    /// [`splice`]'s fixtures, beside the lexer's: pagination artifacts
    /// of each origin, Acrobat's opening with the `Q` that ends the page's
    /// `q`, a range across the two streams, ranges inside text objects,
    /// ranges with white-space between, and forms drawn in and out of
    /// artifacts.
    fn splice_fixtures() -> Vec<Vec<u8>> {
        [
            &b"q /Artifact /P0 BDC q BT /F1 9 Tf (Head) Tj ET Q EMC Q 0 0 9 9 re f"[..],
            b"q 0 g 0 0 9 9 re f 1 1 8 8 re f 2 2 7 7 re f 3 3 6 6 re f 0 0 1 rg 9 9 m 4 4 l h S  \
              /Artifact <</Type /Pagination /Subtype /Header>> BDC Q q 1 0 0 1 9 9 cm /X0 Do Q EMC",
            b"0 g/A <<>> BDC EMC  /B <<>> BDC EMC 1 g 0 0 9 9 re f 1 1 8 8 re f 2 2 7 7 re f",
            b"/Artifact <</Type /Pagination /Subtype /Header>> BDC q /X0 Do Q EMC /X1 Do",
            b"BT /F1 12 Tf 72 700 Td /Artifact <</Subtype /Footer>> BDC (f) Tj EMC \
              0 -14 Td (Body) Tj ET",
            b"/Artifact <</Subtype /Watermark /SHEF_Gen (g1)>> BDC q 0.5 g \
              BT /F1 9 Tf (wm) Tj ET Q EMC /Span <</MCID 0>> BDC (x) Tj EMC",
            b"1 g /Artifact <</Subtype /Background>> BDC 0 0 m 9 9 l S \
              BI /W 2 /H 1 /BPC 8 /CS /CS3 ID ab EIxyz EI EMC Q",
            b"/Artifact <</Type/Pagination/Subtype/Background/SHEF_Gen (g1)\
              /SHEF_OcrBlock({\"path\":\"/a (b)\"}) % kept\n/D <</SHEF_Gen 1>>>> BDC\n\
              q\n0.5 g 0 0 9 9 re f\nQ\nEMC\n",
        ]
        .map(<[u8]>::to_vec)
        .into_iter()
        .chain(content_fixtures())
        .collect()
    }

    /// [`interp`]'s fixtures, beside the splice's: the content
    /// interpreter's pages' content (`content::fixture`), each operator of
    /// the text state, text in each kind of font, forms, images, paths,
    /// clips and marked content; but the chains', the fan-out's and the
    /// inherited form's, which only draw a form the fuzz page hasn't got.
    fn interp_fixtures() -> Vec<Vec<u8>> {
        let pages = crate::content::fixture::every().into_iter();
        let contents = pages.map(|(_, f)| f.content().into_bytes());
        contents
            .filter(|c| c != b"/F Do" && c != b"/Fm Do")
            .collect()
    }

    /// [`interp`]'s fixtures with the fonts' parts ([`FONT_PART`]): the
    /// content of each operator of the text state and of each kind of
    /// font, then the fuzz page's own CMap and `a` CharProc, a `/W`, a
    /// `/FontMatrix`, and a program of each format (`content::fixture`'s
    /// TrueType, CFF and Type 1), so mutations reach the fonts' readers.
    fn font_part_fixtures() -> Vec<Vec<u8>> {
        let programs = [
            crate::content::fixture::truetype_program(),
            crate::content::fixture::cff_program(),
            crate::content::fixture::type1_program(),
        ];
        with_font_parts(&programs)
    }

    /// [`font_part_fixtures`] with programs that once stopped the fuzzer:
    /// a Type 1 program whose integers run past what sheaf hands
    /// read-fonts, and an SFNT whose `loca` record is past its end.
    fn hostile_program_fixtures() -> Vec<Vec<u8>> {
        let programs = [
            crate::content::fixture::type1_integers_program(300),
            crate::content::fixture::loca_past_end_program(),
        ];
        with_font_parts(&programs)
    }

    /// The content of each operator of the text state and of each kind of
    /// font, then the fuzz page's fonts' parts, `programs` each in turn
    /// the last ([`font_part_fixtures`]).
    fn with_font_parts(programs: &[Vec<u8>]) -> Vec<Vec<u8>> {
        let parts: [&[u8]; 4] = [
            b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
              /CMapName /OneByte def\n\
              1 begincodespacerange <00> <FF> endcodespacerange\n\
              1 begincidrange <20> <7E> 1 endcidrange\n\
              endcmap CMapName currentdict /CMap defineresource pop end end",
            b"1000 0 0 -200 1000 800 d1 0 0 1000 800 re f",
            b"[1 [400 500] 10 20 700]",
            b"[0.001 0 0 0.001 0 0]",
        ];
        let pages = [
            crate::content::fixture::operators(),
            crate::content::fixture::fonts(),
        ];
        (pages.iter())
            .flat_map(|f| programs.iter().map(move |program| (f, program)))
            .map(|(f, program)| {
                let mut data = f.content().into_bytes();
                for part in parts.into_iter().chain([&program[..]]) {
                    data.extend(FONT_PART);
                    data.extend(part);
                }
                data
            })
            .collect()
    }

    /// [`op_json`]'s fixtures: every op, between them every form of every
    /// reference and every kind of mark (`sheaf_edit::fixture`), as one
    /// envelope, and each op as an envelope of its own; then
    /// [`applying`]'s ops, which all apply.
    fn op_json_fixtures() -> Vec<Vec<u8>> {
        let ops = sheaf_edit::fixture::every_op();
        let envelope =
            |ops| serde_json::to_vec(&sheaf_edit::Envelope::new(None, ops)).expect("Op JSON");
        let one = ops.iter().map(|op| envelope(vec![op.clone()]));
        (std::iter::once(envelope(ops.clone())).chain(one))
            .chain(std::iter::once(envelope(applying())))
            .collect()
    }

    /// Ops of every kind, and marks of every kind (the fixture's), that
    /// apply to [`op_json`]'s document one after another, each naming what
    /// it needs as the fixture's ops do: by position, title path, `/NM`,
    /// local and grant. Between them they reach each cascade.
    fn applying() -> Vec<sheaf_edit::Op> {
        use sheaf_edit::{
            Anchor, AnnotRef, BlobSel, BookmarkPatch, BookmarkRef, BoxSpec, Delta, FontChoice,
            GridPos, Local, Margins, MarkKind, MarkPatch, NewMark, Op, PageLabelRange, PageRef,
            PageSel, PageSource, Patch, ScopeSel, Sibling, Turn, Vec2, WatermarkContent,
            WatermarkSpec, Z,
        };
        let pos = PageRef::Pos;
        let positions = |s: &str| PageSel::Positions(s.parse().unwrap());
        let path =
            |titles: &[&str]| BookmarkRef::Path(titles.iter().map(|t| t.to_string()).collect());
        let nm = |nm: &str| AnnotRef::Nm {
            nm: nm.into(),
            page: None,
        };
        let anchor = |page, x| Anchor {
            page: pos(page),
            rect: crate::annot::Box2 {
                x,
                y: 220.0,
                w: 22.0,
                h: 22.0,
            },
        };
        let file = |file, name: &str| PageSource::File {
            file,
            name: name.into(),
        };
        let every = sheaf_edit::fixture::every_op();
        let spec = every.iter().find_map(|op| match op {
            Op::SetHeaderFooter { spec, .. } => Some(spec.clone()),
            _ => None,
        });
        let style = every.iter().find_map(|op| match op {
            Op::AddConnector { style, .. } => Some(style.clone()),
            _ => None,
        });
        let mut ops = vec![
            Op::AddBookmark {
                parent: None,
                at: Sibling::Last,
                title: "Added".into(),
                target: Some(pos(2)),
                view: None,
                color: None,
                style: Default::default(),
                local: Some(Local(1)),
            },
            Op::AddBookmark {
                parent: Some(BookmarkRef::New(Local(1))),
                at: Sibling::First,
                title: "Under it".into(),
                target: Some(pos(3)),
                view: None,
                color: Some([0, 0, 255]),
                style: Default::default(),
                local: None,
            },
            Op::PatchBookmarks {
                bookmarks: vec![path(&["Part I", "Ch. 2"])],
                patch: BookmarkPatch {
                    title: Some("Renamed".into()),
                    target: Patch::Set(pos(4)),
                    ..Default::default()
                },
            },
            Op::MoveBookmarks {
                bookmarks: vec![path(&["Appendix"])],
                parent: Some(BookmarkRef::New(Local(1))),
                at: Sibling::Last,
                with_pages: true,
            },
            Op::AddConnector {
                a: anchor(1, 500.0),
                b: anchor(3, 100.0),
                label: "1".into(),
                style: style.expect("the fixture's connector"),
                locals: vec![Local(2), Local(3)],
            },
            Op::PatchMarks {
                marks: vec![nm("sheaf:12")],
                patch: MarkPatch {
                    fill: Patch::Set([1, 2, 3]),
                    contents: Some("<<name>> on <<date>>".into()),
                    ..Default::default()
                },
                imported: None,
            },
            Op::MoveMarks {
                marks: vec![AnnotRef::New(Local(2))],
                by: Delta::View(Vec2 { x: 5.0, y: 5.0 }),
                to_page: None,
            },
            Op::MoveMarks {
                marks: vec![nm("sheaf:12")],
                by: Delta::Page(Vec2 { x: 1.0, y: 1.0 }),
                to_page: Some(pos(1)),
            },
            Op::AdoptAnnotations {
                annots: vec![nm("acro-1")],
            },
            Op::DeleteAnnotations {
                annots: vec![AnnotRef::New(Local(3)), nm("sheaf:2")],
            },
            Op::SetOcrText {
                page: pos(1),
                layer: sheaf_edit::fixture::layer(),
            },
            Op::ClearOcrText {
                pages: positions("1-2"),
            },
            Op::SetPageLabels {
                ranges: Some(vec![PageLabelRange {
                    start: pos(3),
                    style: sheaf_edit::LabelStyle::UpperLetters,
                    prefix: "App-".into(),
                    first: 1,
                }]),
            },
            Op::SetHeaderFooter {
                spec: spec.expect("the fixture's header"),
                scope: ScopeSel::Pages(positions("1-3")),
            },
            Op::RemoveHeaderFooter {
                pages: positions("2"),
                include_authored: false,
            },
            Op::SetWatermark {
                spec: WatermarkSpec {
                    content: WatermarkContent::Text {
                        text: "Café 機密".into(),
                        font: FontChoice::TimesRoman,
                        size: 60.0,
                        color: [200, 30, 30],
                        opacity: 0.3,
                        angle: 45.0,
                    },
                    position: GridPos::Center,
                },
                z: Z::Over,
                scope: ScopeSel::All,
            },
            Op::SetWatermark {
                spec: WatermarkSpec {
                    content: WatermarkContent::Page {
                        pdf: Some(BlobSel::With("logo".into())),
                        page: 1,
                        scale: 0.5,
                        opacity: 0.5,
                    },
                    position: GridPos::BottomRight,
                },
                z: Z::Under,
                scope: ScopeSel::Pages(positions("1-3")),
            },
            Op::RemoveWatermark {
                z: Some(Z::Under),
                pages: positions("2"),
                include_authored: false,
            },
            Op::SetPageBoxes {
                pages: positions("2"),
                crop: Patch::Set(BoxSpec::Margins(Margins {
                    top: 36.0,
                    bottom: 36.0,
                    left: 18.0,
                    right: 18.0,
                })),
                trim: Patch::Keep,
                bleed: Patch::Keep,
                art: Patch::Set(BoxSpec::Rect(crate::annot::Box2 {
                    x: 72.0,
                    y: 72.0,
                    w: 288.0,
                    h: 144.0,
                })),
            },
            Op::SetPageBoxes {
                pages: positions("1"),
                crop: Patch::Keep,
                trim: Patch::Clear,
                bleed: Patch::Keep,
                art: Patch::Keep,
            },
        ];
        // The fixture's marks of every kind on the first page; a reply's
        // and a status's to the other tool's comment on the fourth; and a
        // page stamp's and an attachment's holding the files granted.
        for op in every {
            let Op::AddMark { mark, .. } = op else {
                continue;
            };
            let (page, reply_to, asset) = match mark.kind {
                MarkKind::Reply {} | MarkKind::Status { .. } => (4, Some(nm("a1b2-c3")), None),
                MarkKind::PageStamp { .. } => (1, None, Some(BlobSel::Asset(0))),
                MarkKind::FileAttachment { .. } => (1, None, Some(BlobSel::Asset(1))),
                _ => (1, None, None),
            };
            ops.push(Op::AddMark {
                page: pos(page),
                mark: NewMark { asset, ..mark },
                reply_to,
                local: None,
                imported: None,
            });
        }
        // Then pages turned, copied, brought in, replaced and moved; and
        // some go, with what's on them, their fields, the bookmarks to
        // them, a label range's start and the header's pages.
        ops.extend([
            Op::RotatePages {
                pages: PageSel::Span {
                    first: pos(1),
                    last: pos(2),
                },
                by: Turn::Quarter,
            },
            Op::DuplicatePages {
                pages: positions("1,3"),
                locals: vec![Local(4), Local(5)],
            },
            Op::InsertPages {
                from: file(BlobSel::Asset(2), "scan.pdf"),
                pages: Some("1-2".parse().unwrap()),
                before: Some(PageRef::New(Local(4))),
                bring: Default::default(),
                locals: vec![Local(6), Local(7)],
            },
            Op::InsertPages {
                from: file(BlobSel::With("logo".into()), "logo.pdf"),
                pages: None,
                before: None,
                bring: Default::default(),
                locals: Vec::new(),
            },
            Op::ReplacePages {
                pages: vec![PageRef::New(Local(6))],
                from: file(BlobSel::Asset(2), "scan.pdf"),
                with: Some("2".parse().unwrap()),
                bring: Default::default(),
            },
            Op::MovePages {
                pages: PageSel::Ids(vec![PageRef::New(Local(5))]),
                before: Some(pos(1)),
            },
            Op::DeleteBookmarks {
                bookmarks: vec![BookmarkRef::New(Local(1))],
            },
            Op::DeletePages {
                pages: positions("2-4"),
            },
        ]);
        ops
    }

    /// The fixtures, as the fuzz targets' first corpus, into the directory
    /// `SHEAF_FUZZ_SEEDS` names: `fuzz.yml` seeds each night's run with
    /// them, so the repository holds no PDF. The target
    /// `SHEAF_FUZZ_TARGET` names takes the marks' fixtures when it's
    /// `sheaf_annot`, the content when it's `lexer`, `splice` or
    /// `interp` (the interpreter's pages' too), Op JSON when it's
    /// `op_json`, files sheaf saved with watermarks when it's
    /// `sheaf_generators`, and the files otherwise.
    #[test]
    #[ignore = "fuzz.yml's seeds"]
    fn write_fuzz_seeds() {
        let dir = std::env::var_os("SHEAF_FUZZ_SEEDS").expect("SHEAF_FUZZ_SEEDS names a directory");
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        let named = |seeds: Vec<Vec<u8>>, name: fn(usize) -> String| {
            seeds
                .into_iter()
                .enumerate()
                .map(move |(n, seed)| (name(n), seed))
        };
        let seeds: Vec<(String, Vec<u8>)> = match std::env::var("SHEAF_FUZZ_TARGET").as_deref() {
            Ok("sheaf_annot") => named(mark_fixtures(), |n| format!("marks-{n}")).collect(),
            Ok("lexer") => named(content_fixtures(), |n| format!("content-{n}")).collect(),
            Ok("splice") => named(splice_fixtures(), |n| format!("content-{n}")).collect(),
            Ok("interp") => {
                let content = splice_fixtures().into_iter().chain(interp_fixtures());
                let content = content.chain(font_part_fixtures());
                let content = content.chain(hostile_program_fixtures());
                named(content.collect(), |n| format!("content-{n}")).collect()
            }
            Ok("op_json") => named(op_json_fixtures(), |n| format!("ops-{n}.json")).collect(),
            Ok("sheaf_generators") => {
                named(generator_fixtures(), |n| format!("generators-{n}.pdf")).collect()
            }
            _ => named(fixtures(), |n| format!("fixture-{n}.pdf")).collect(),
        };
        for (name, seed) in seeds {
            std::fs::write(dir.join(name), seed).unwrap();
        }
    }

    /// Unmutated, each font-part fixture's last part is `/TT`'s program,
    /// which reads: the fuzzer starts from a program of each format.
    #[test]
    fn the_font_part_fixtures_give_a_program_that_reads() {
        let page = content::fixture::fuzz_page();
        for fixture in font_part_fixtures() {
            let parts = font_parts(&fixture);
            assert_eq!(parts.len(), 6);
            let page = with_fonts(&page, &parts[1..]);
            let fonts = page
                .resources()
                .unwrap()
                .as_dict()
                .unwrap()
                .get(b"Font")
                .unwrap();
            let tt = fonts.as_dict().unwrap().get(b"TT").unwrap();
            let read = content::Fonts::default();
            let mut budget = Limits::DEFAULT.content.decompressed;
            let tt = read
                .get(&page.pdf, tt, &Limits::DEFAULT, &mut budget)
                .unwrap()
                .unwrap();
            assert_eq!(tt.program(), content::FontProgram::Read);
            // Its bytes, and what drawing its glyphs costs.
            assert!(Limits::DEFAULT.content.decompressed - budget > parts[5].len());
        }
    }

    /// Unmutated, the marks' fixtures read as marks, every one, by their
    /// digests.
    #[test]
    fn the_mark_fixtures_read_as_marks() {
        for fixture in mark_fixtures() {
            let src = annotated_page(&fixture).unwrap();
            let page = src.page(0).unwrap();
            let read = (page.annots.iter()).filter_map(|e| crate::file::sheaf_mark(src.pdf(), e));
            assert_eq!(read.count(), every_mark_kind().len());
        }
    }

    #[test]
    fn fuzz_bodies_survive_mutated_fixtures() {
        let fixtures = fixtures();
        let started = Instant::now();
        let mut rng = fastrand::Rng::with_seed(0x5eaf_f022);
        for (n, fixture) in fixtures.iter().enumerate() {
            // Unmutated, each opens: the mutations start from a file.
            let kind = BlobKind::file(Path::new("fixture.pdf"), None);
            let src = Store::new().open(fixture.as_slice().into(), kind).unwrap();
            assert!(src.page_count() > 0);
            for case in 0..CASES {
                let mut data = fixture.clone();
                for _ in 0..rng.usize(1..=4) {
                    mutate(&mut rng, &mut data, TOKENS);
                }
                let ran = std::panic::catch_unwind(|| {
                    lazy_reader(&data);
                    page_tree(&data);
                });
                // The seed alone can't replay a case of the AES fixtures,
                // whose IVs are drawn afresh each run: the input is kept.
                if let Err(panic) = ran {
                    let kept = std::env::temp_dir().join(format!("sheaf-fuzz-{n}-{case}.pdf"));
                    std::fs::write(&kept, &data).unwrap();
                    eprintln!("fixture {n}, case {case}: the input is {}", kept.display());
                    std::panic::resume_unwind(panic);
                }
            }
        }
        for (n, fixture) in mark_fixtures().iter().enumerate() {
            for case in 0..CASES {
                let mut data = fixture.clone();
                for _ in 0..rng.usize(1..=4) {
                    mutate(&mut rng, &mut data, TOKENS);
                }
                let ran = std::panic::catch_unwind(|| sheaf_annot(&data));
                if let Err(panic) = ran {
                    let kept = std::env::temp_dir().join(format!("sheaf-fuzz-marks-{n}-{case}"));
                    std::fs::write(&kept, &data).unwrap();
                    eprintln!("marks {n}, case {case}: the input is {}", kept.display());
                    std::panic::resume_unwind(panic);
                }
            }
        }
        for (n, fixture) in generator_fixtures().iter().enumerate() {
            for case in 0..CASES {
                let mut data = fixture.clone();
                for _ in 0..rng.usize(1..=4) {
                    mutate(&mut rng, &mut data, TOKENS);
                }
                let ran = std::panic::catch_unwind(|| sheaf_generators(&data));
                if let Err(panic) = ran {
                    let kept = std::env::temp_dir().join(format!("sheaf-fuzz-gens-{n}-{case}.pdf"));
                    std::fs::write(&kept, &data).unwrap();
                    eprintln!(
                        "generators {n}, case {case}: the input is {}",
                        kept.display()
                    );
                    std::panic::resume_unwind(panic);
                }
            }
        }
        for (n, fixture) in splice_fixtures().iter().enumerate() {
            // Unmutated, each lexes, and those with marked content splice.
            let res = ResCx::new([(b"CS3".to_vec(), 3)]);
            assert!(crate::content::lex(fixture, &res, &Limits::DEFAULT).all(|t| t.is_ok()));
            let marks = fixture.windows(3).any(|w| w == b"BDC");
            assert_eq!(splice(fixture) > 0, marks, "fixture {n}");
            for case in 0..CASES {
                let mut data = fixture.clone();
                for _ in 0..rng.usize(1..=4) {
                    mutate(&mut rng, &mut data, CONTENT_TOKENS);
                }
                let ran = std::panic::catch_unwind(|| {
                    lexer(&data);
                    splice(&data);
                    interp(&data);
                });
                if let Err(panic) = ran {
                    let kept = std::env::temp_dir().join(format!("sheaf-fuzz-content-{n}-{case}"));
                    std::fs::write(&kept, &data).unwrap();
                    eprintln!("content {n}, case {case}: the input is {}", kept.display());
                    std::panic::resume_unwind(panic);
                }
            }
        }
        let pages = interp_fixtures().into_iter().chain(font_part_fixtures());
        for (n, fixture) in pages.chain(hostile_program_fixtures()).enumerate() {
            // Unmutated, each draws on the fuzz page.
            assert!(interp(&fixture) > 0, "page {n}");
            for case in 0..INTERP_CASES {
                let mut data = fixture.clone();
                for _ in 0..rng.usize(1..=4) {
                    mutate(&mut rng, &mut data, CONTENT_TOKENS);
                }
                if let Err(panic) = std::panic::catch_unwind(|| interp(&data)) {
                    let kept = std::env::temp_dir().join(format!("sheaf-fuzz-page-{n}-{case}"));
                    std::fs::write(&kept, &data).unwrap();
                    eprintln!("page {n}, case {case}: the input is {}", kept.display());
                    std::panic::resume_unwind(panic);
                }
            }
        }
        // Unmutated, the ops made to apply do, all of them.
        let fixtures = op_json_fixtures();
        assert_eq!(op_json(fixtures.last().unwrap()), applying().len());
        for (n, fixture) in fixtures.iter().enumerate() {
            let json: serde_json::Value = serde_json::from_slice(fixture).unwrap();
            let cases = match n + 1 == fixtures.len() {
                true => 10 * OP_CASES,
                false => OP_CASES,
            };
            for case in 0..cases {
                // Mostly JSON still, which reaches lowering; now and then
                // bytes, which reach the reader.
                let mut data = match rng.u8(..4) {
                    0 => fixture.clone(),
                    _ => {
                        let mut json = json.clone();
                        for _ in 0..rng.usize(1..=3) {
                            mutate_json(&mut rng, &mut json);
                        }
                        serde_json::to_vec(&json).unwrap()
                    }
                };
                for _ in 0..rng.usize(0..=1) {
                    mutate(&mut rng, &mut data, JSON_TOKENS);
                }
                let ran = std::panic::catch_unwind(|| op_json(&data));
                if let Err(panic) = ran {
                    let kept = std::env::temp_dir().join(format!("sheaf-fuzz-ops-{n}-{case}.json"));
                    std::fs::write(&kept, &data).unwrap();
                    eprintln!("ops {n}, case {case}: the input is {}", kept.display());
                    std::panic::resume_unwind(panic);
                }
            }
        }
        let took = started.elapsed();
        assert!(took < Duration::from_secs(60), "{took:?}");
    }
}
