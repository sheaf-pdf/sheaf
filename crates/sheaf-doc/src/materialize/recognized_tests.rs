//! Watermarks and backgrounds a file holds, once it opens again (M10.5
//! slice 16): found from the catalog's `/SheafGenerators` and the pages'
//! blocks, laid out alike again where nothing changed, set aside where
//! something did or they're gone; another file's kept, isolated, until a
//! filter takes them out.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use in_progress::NoProgress;

use lopdf::{Dictionary, Document as Pdf, Object, ObjectId, Stream, dictionary};
use sheaf_edit::{GridPos, Op, Turn, WatermarkContent, Z};

use super::append_tests::{appended_in_place, keys_at, planned_in_place, square, written};
use super::key::ObjKey;
use super::removal_tests::{opened, scratch, text_file};
use super::watermark_tests::{image_watermark, kinds, png};
use super::{AskAnswers, How, StrategyKind, save_rewritten};
use crate::content::{Balance, Concat, GenId, ResCx};
use crate::model::{
    Document, Filter, ImageRef, MarkedSel, PageId, Scope, StreamSel, WatermarkContent as Content,
};
use crate::realize::ViewInputs;
use crate::source::Limits;
use crate::tx::fixture as fx;

/// A watermark of `text`, over every page or `pages`.
fn watermark(text: &str, pages: Option<&[PageId]>) -> Op {
    fx::set_watermark(fx::watermark(text), Z::Over, pages)
}

/// `doc` written `how`, opened as `name` in `dir`.
pub(super) fn reopened(dir: &Path, name: &str, doc: &Document, how: How) -> (PathBuf, Document) {
    opened(dir, name, &how.write(doc))
}

/// Every page of `pdf`, in order.
pub(super) fn pages(pdf: &Pdf) -> Vec<ObjectId> {
    pdf.get_pages().into_values().collect()
}

/// What a page of `pdf` names in its `/Contents`, in order.
pub(super) fn contents(pdf: &Pdf, page: ObjectId) -> Vec<ObjectId> {
    let page = pdf.get_dictionary(page).unwrap();
    match page.get(b"Contents").unwrap() {
        Object::Array(a) => a.iter().map(|e| e.as_reference().unwrap()).collect(),
        Object::Reference(r) => vec![*r],
        other => panic!("contents {other:?}"),
    }
}

/// Stream `id` of `pdf`: its dictionary and its data decoded.
pub(super) fn stream(pdf: &Pdf, id: ObjectId) -> (Dictionary, Vec<u8>) {
    let s = pdf.get_object(id).unwrap().as_stream().unwrap();
    let data = s
        .decompressed_content()
        .unwrap_or_else(|_| s.content.clone());
    (s.dict.clone(), data)
}

/// The generator stream `id` of `pdf` is a block of, if it's one.
pub(super) fn block_of(pdf: &Pdf, id: ObjectId) -> Option<GenId> {
    let (dict, _) = stream(pdf, id);
    let marker = dict.get(b"SheafBlock").ok()?.as_dict().ok()?;
    GenId::parse(marker.get(b"G").ok()?.as_str().ok()?)
}

/// Whether stream `id` of `pdf` isolates a page's content.
pub(super) fn isolates(pdf: &Pdf, id: ObjectId) -> bool {
    let (dict, _) = stream(pdf, id);
    (dict.get(b"SheafBlock").and_then(Object::as_dict))
        .is_ok_and(|m| m.get(b"K").and_then(Object::as_name).ok() == Some(b"Isolate"))
}

/// Each page's blocks, by generator, in order.
pub(super) fn blocks(pdf: &Pdf) -> Vec<Vec<GenId>> {
    (pages(pdf).into_iter())
        .map(|p| {
            (contents(pdf, p).into_iter())
                .filter_map(|c| block_of(pdf, c))
                .collect()
        })
        .collect()
}

/// The page's content, its streams decoded and joined.
pub(super) fn drawn(pdf: &Pdf, page: ObjectId) -> String {
    (contents(pdf, page).into_iter())
        .map(|c| String::from_utf8_lossy(&stream(pdf, c).1).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The highest the running `q` count goes through a page's content, and
/// its balance.
pub(super) fn depth(pdf: &Pdf, page: ObjectId) -> (i32, Balance) {
    let data: Vec<Vec<u8>> = (contents(pdf, page).into_iter())
        .map(|c| stream(pdf, c).1)
        .collect();
    let streams: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
    let concat = Concat::new(&streams);
    let lim = Limits::DEFAULT;
    let (mut now, mut deepest) = (0, 0);
    for token in crate::content::lex(concat.bytes(), &ResCx::default(), &lim) {
        match token.unwrap().1 {
            crate::content::Tok::Op(b"q") => now += 1,
            crate::content::Tok::Op(b"Q") => now -= 1,
            _ => {}
        }
        deepest = deepest.max(now);
    }
    let balance = crate::content::balance(&concat, &ResCx::default(), &lim).unwrap();
    (deepest, balance)
}

/// What every object of `pdf` says, as one string: names, strings and
/// streams decoded.
pub(super) fn everything(pdf: &Pdf) -> String {
    let mut out = String::new();
    for obj in pdf.objects.values() {
        out.push_str(&format!("{obj:?}"));
        if let Object::Stream(s) = obj {
            let data = s
                .decompressed_content()
                .unwrap_or_else(|_| s.content.clone());
            out.push_str(&String::from_utf8_lossy(&data));
        }
    }
    out
}

/// A watermark over every page and a background under the second, saved
/// and opened again, are the generators they were: their ids, what they
/// draw, where, their z and what they replace; the watermark on every page,
/// the background on the page that holds its block. The source says so.
/// Both ways.
fn found(how: How) {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let second = doc.page(1).unwrap();
    let under = fx::set_watermark(fx::watermark("under"), Z::Under, Some(&[second]));
    doc.commit_ops(vec![watermark("DRAFT", None), under], "watermark")
        .unwrap();
    let (_, again) = reopened(&dir, "b.pdf", &doc, how);
    let (was, now) = (doc.state(), again.state());
    for z in [Z::Over, Z::Under] {
        let (a, b) = (was.watermark(z).unwrap(), now.watermark(z).unwrap());
        assert_eq!(a.id, b.id, "{how:?}");
        let (a, b) = (a.spec.watermark().unwrap(), b.spec.watermark().unwrap());
        let said = |w: &crate::model::WatermarkGen| {
            (w.content.clone(), w.position, w.z, w.replaces.clone())
        };
        assert_eq!(said(a), said(b), "{how:?}");
    }
    let scope = |z| now.watermark(z).unwrap().scope().into_owned();
    assert_eq!(scope(Z::Over), Scope::All);
    let second = again.page(1).unwrap();
    assert_eq!(scope(Z::Under), Scope::Pages(im::OrdSet::unit(second)));
    let saved = again.primary().generators();
    let pages: BTreeSet<Option<Vec<u32>>> = saved.iter().map(|s| s.pages.clone()).collect();
    assert_eq!(pages, BTreeSet::from([None, Some(vec![1])]), "{how:?}");
    assert_eq!(now.check_invariants(), Ok(()));
}

crate::both_ways!(
    a_saved_watermark_is_found_again,
    a_saved_watermark_is_found_again_appended: found
);

/// A file saved with a watermark and opened again is written as it is: no
/// block, no isolation and no resources anew, each page's contents and
/// resources the file's; and its pages are untouched under the screen's
/// inputs, though not under OCR's, which leave the watermark out.
fn as_it_is(how: How) {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let under = fx::set_watermark(fx::watermark("under"), Z::Under, None);
    doc.commit_ops(vec![watermark("DRAFT", None), under], "watermark")
        .unwrap();
    let (_, again) = reopened(&dir, "b.pdf", &doc, how);
    let file = Pdf::load_mem(again.primary().bytes()).unwrap();
    let written = Pdf::load_mem(&How::Rewritten.write(&again)).unwrap();
    for (a, b) in pages(&file).into_iter().zip(pages(&written)) {
        assert_eq!(contents(&file, a), contents(&written, b), "{how:?}");
        let res = |pdf: &Pdf, p| pdf.get_dictionary(p).unwrap().get(b"Resources").cloned();
        assert_eq!(res(&file, a).unwrap(), res(&written, b).unwrap(), "{how:?}");
    }
    let st = again.state();
    for (id, _, _) in st.pages() {
        assert!(
            crate::realize::untouched(st, id, &ViewInputs::screen()),
            "{how:?}"
        );
        let content = ViewInputs::content_only();
        assert!(!crate::realize::untouched(st, id, &content), "{how:?}");
    }
}

crate::both_ways!(
    a_reopened_watermark_is_written_as_it_is,
    a_reopened_watermark_is_written_as_it_is_appended: as_it_is
);

/// A watermark found again and changed keeps its generator, and each page
/// holds one block of it, the new one: the old is set aside, and so are
/// the streams that isolated the page's content, which are written again
/// around it. Both ways.
fn updated(how: How) {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
        .unwrap();
    let id = doc.state().watermark(Z::Over).unwrap().id;
    let (_, mut again) = reopened(&dir, "b.pdf", &doc, How::Rewritten);
    again
        .commit_ops(vec![watermark("FINAL", None)], "watermark")
        .unwrap();
    assert_eq!(again.state().watermark(Z::Over).unwrap().id, id);
    let pdf = Pdf::load_mem(&how.write(&again)).unwrap();
    assert_eq!(blocks(&pdf), vec![vec![id]; 3], "{how:?}");
    for page in pages(&pdf) {
        let ids = contents(&pdf, page);
        let isolating = ids.iter().filter(|&&c| isolates(&pdf, c)).count();
        assert_eq!((ids.len(), isolating), (4, 2), "{how:?}");
        let text = drawn(&pdf, page);
        assert!(
            text.contains("(FINAL)") && !text.contains("DRAFT"),
            "{how:?}: {text}"
        );
        assert_eq!(depth(&pdf, page).1, Balance::default());
    }
    let (_, third) = opened(&dir, "c.pdf", &how.write(&again));
    let wm = third.state().watermark(Z::Over).unwrap();
    assert_eq!(wm.id, id);
    let text = |g: &crate::model::Generator| match &g.spec.watermark().unwrap().content {
        Content::Text { text, .. } => text.to_string(),
        other => panic!("{other:?}"),
    };
    assert_eq!(text(wm), "FINAL");
}

crate::both_ways!(
    a_found_watermark_changed_is_one_block_a_page,
    a_found_watermark_changed_is_one_block_a_page_appended: updated
);

/// A watermark and a background found again and removed leave nothing of
/// theirs: the save rewrites, and the file it writes has no block, no
/// stream isolating content, no `/SheafGenerators`, no property list
/// naming a generator, and nowhere the watermark's text.
#[test]
fn a_found_watermark_removed_leaves_no_trace() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let under = fx::set_watermark(fx::watermark("UNDERNEATH"), Z::Under, None);
    doc.commit_ops(vec![watermark("DRAFT", None), under], "watermark")
        .unwrap();
    let (path, mut again) = reopened(&dir, "b.pdf", &doc, How::Rewritten);
    let all: Vec<PageId> = again.state().order().iter().copied().collect();
    again
        .commit_ops(vec![fx::remove_watermark(None, &all)], "remove")
        .unwrap();
    let report = super::save(&mut again, &path, &AskAnswers::default(), &NoProgress).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    let pdf = Pdf::load(&path).unwrap();
    assert_eq!(blocks(&pdf), vec![Vec::<GenId>::new(); 3]);
    for page in pages(&pdf) {
        assert_eq!(contents(&pdf, page).len(), 1);
        assert!(drawn(&pdf, page).contains("(page "));
    }
    let all = everything(&pdf);
    for gone in ["DRAFT", "UNDERNEATH", "SheafBlock", "SheafGen", "Isolate"] {
        assert!(!all.contains(gone), "{gone}");
    }
    let (_, third) = opened(&dir, "c.pdf", &std::fs::read(&path).unwrap());
    assert!(third.state().generators().is_empty());
}

/// However a watermark found again goes, none of its blocks stays: taken
/// off some pages and then the rest, by two removes; its one page
/// deleted; or the pages it isn't on extracted.
#[test]
fn a_found_watermark_gone_any_way_leaves_no_block() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let first = doc.page(0).unwrap();
    let under = fx::set_watermark(fx::watermark("one"), Z::Under, Some(&[first]));
    doc.commit_ops(vec![watermark("DRAFT", None), under], "watermark")
        .unwrap();
    let bytes = How::Rewritten.write(&doc);
    // Two removes, the second taking it off the pages the first left it on.
    let (_, mut again) = opened(&dir, "b.pdf", &bytes);
    let p: Vec<PageId> = again.state().order().iter().copied().collect();
    let removes = [
        fx::remove_watermark(Some(Z::Over), &p[..2]),
        fx::remove_watermark(Some(Z::Over), &p[2..]),
    ];
    for remove in removes {
        again.commit_ops(vec![remove], "remove").unwrap();
        let pdf = Pdf::load_mem(&How::Rewritten.write(&again)).unwrap();
        let over = again.state().watermark(Z::Over).map(|g| g.id);
        for (i, on) in blocks(&pdf).into_iter().enumerate() {
            let want: Vec<GenId> = (again.state().generators().values())
                .filter(|g| g.covers(p[i]))
                .map(|g| g.id)
                .collect();
            let mut on = on;
            on.sort();
            let mut want = want;
            want.sort();
            assert_eq!(on, want, "{i}, {over:?}");
        }
    }
    // The background's one page deleted.
    let (_, mut again) = opened(&dir, "c.pdf", &bytes);
    let first = again.page(0).unwrap();
    again
        .commit_ops(vec![fx::delete(&[first])], "delete")
        .unwrap();
    assert!(again.state().watermark(Z::Under).is_none());
    let pdf = Pdf::load_mem(&How::Rewritten.write(&again)).unwrap();
    let over = again.state().watermark(Z::Over).unwrap().id;
    assert_eq!(blocks(&pdf), vec![vec![over]; 2]);
    // The pages the background isn't on extracted.
    let (_, again) = opened(&dir, "d.pdf", &bytes);
    let rest: Vec<PageId> = again.state().order().iter().skip(1).copied().collect();
    let pdf =
        Pdf::load_mem(&crate::save::extract_bytes(&again.snapshot(), &rest).unwrap()).unwrap();
    assert_eq!(blocks(&pdf), vec![vec![over]; 2]);
    let all = everything(&pdf);
    assert!(!all.contains("(one)"), "the background's text");
}

/// An image watermark found again draws the image its blocks draw, kept
/// as the file has it: moved, the file still holds one image, the one it
/// held, which nothing writes again; then removed, it holds none. Both
/// ways.
fn image(how: How) {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let (_png, spec) = image_watermark(&doc, &png());
    doc.commit_ops(vec![fx::set_watermark(spec, Z::Under, None)], "image")
        .unwrap();
    let (path, mut again) = reopened(&dir, "b.pdf", &doc, How::Rewritten);
    let images = |pdf: &Pdf| -> BTreeSet<ObjectId> {
        (pdf.objects.iter())
            .filter(|(_, o)| {
                (o.as_stream()).is_ok_and(|s| {
                    s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image")
                })
            })
            .map(|(&id, _)| id)
            .collect()
    };
    let held = images(&Pdf::load(&path).unwrap());
    assert_eq!(held.len(), 1);
    let wm = again.state().watermark(Z::Under).unwrap();
    let Content::Image {
        image: ImageRef::Placed(placed),
        ..
    } = &wm.spec.watermark().unwrap().content
    else {
        panic!("{:?}", wm.spec);
    };
    assert_eq!(BTreeSet::from([placed.at.id]), held);
    let moved = sheaf_edit::WatermarkSpec {
        content: WatermarkContent::Image {
            image: None,
            scale: 0.5,
            opacity: 0.5,
        },
        position: GridPos::Top,
    };
    again
        .commit_ops(vec![fx::set_watermark(moved, Z::Under, None)], "move")
        .unwrap();
    let pdf = Pdf::load_mem(&how.write(&again)).unwrap();
    let used: BTreeSet<ObjectId> = (pages(&pdf).into_iter())
        .flat_map(|p| {
            let res = pdf.get_dictionary(p).unwrap().get(b"Resources").unwrap();
            let res = pdf.dereference(res).unwrap().1.as_dict().unwrap().clone();
            let xobjects = res.get(b"XObject").unwrap();
            let xobjects = pdf
                .dereference(xobjects)
                .unwrap()
                .1
                .as_dict()
                .unwrap()
                .clone();
            xobjects
                .iter()
                .map(|(_, r)| r.as_reference().unwrap())
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(used, held, "{how:?}");
    if how == How::Rewritten {
        assert_eq!(images(&pdf), held, "{how:?}");
    }
    let all: Vec<PageId> = again.state().order().iter().copied().collect();
    again
        .commit_ops(vec![fx::remove_watermark(None, &all)], "remove")
        .unwrap();
    let pdf = Pdf::load_mem(&How::Rewritten.write(&again)).unwrap();
    assert!(images(&pdf).is_empty());
    assert_eq!(blocks(&pdf), vec![Vec::<GenId>::new(); 3]);
}

crate::both_ways!(
    a_found_image_watermark_keeps_its_image,
    a_found_image_watermark_keeps_its_image_appended: image
);

/// A watermark on every page, found again, is on a page inserted after:
/// it has a block of it like the rest. Both ways.
fn inserted(how: How) {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
        .unwrap();
    let (_, mut again) = reopened(&dir, "b.pdf", &doc, How::Rewritten);
    let id = again.state().watermark(Z::Over).unwrap().id;
    let other = super::tests::add(&again, &text_file(1), "other.pdf");
    super::tests::insert(&mut again, &other, 0);
    let pdf = Pdf::load_mem(&how.write(&again)).unwrap();
    assert_eq!(blocks(&pdf), vec![vec![id]; 3], "{how:?}");
}

crate::both_ways!(
    a_found_watermark_on_every_page_is_on_a_page_inserted,
    a_found_watermark_on_every_page_is_on_a_page_inserted_appended: inserted
);

/// Five times opened, watermarked again and saved, a page's content is
/// isolated no deeper than twice, and its pages draw the watermark in one
/// font, one graphics state and one property list each group of them.
#[test]
fn five_cycles_keep_isolation_shallow() {
    let dir = scratch();
    let mut bytes = text_file(3);
    for cycle in 0..5 {
        let (path, mut doc) = opened(&dir, &format!("{cycle}.pdf"), &bytes);
        let set = watermark(&format!("cycle {cycle}"), None);
        let under = fx::set_watermark(fx::watermark(&format!("under {cycle}")), Z::Under, None);
        doc.commit_ops(vec![set, under], "watermark").unwrap();
        match cycle % 2 {
            0 => {
                save_rewritten(&mut doc, &path).unwrap();
            }
            _ => {
                appended_in_place(&mut doc).unwrap();
            }
        }
        bytes = std::fs::read(&path).unwrap();
        let pdf = Pdf::load_mem(&bytes).unwrap();
        let mut reached = BTreeSet::new();
        for page in pages(&pdf) {
            let (deepest, balance) = depth(&pdf, page);
            assert!(deepest <= 2, "{cycle}: {deepest}");
            assert_eq!(balance, Balance::default());
            assert_eq!(contents(&pdf, page).len(), 5, "{cycle}");
            let res = pdf.get_dictionary(page).unwrap().get(b"Resources").unwrap();
            let res = pdf.dereference(res).unwrap().1.as_dict().unwrap().clone();
            for kind in [b"Font".as_slice(), b"ExtGState", b"Properties"] {
                let category = pdf.dereference(res.get(kind).unwrap()).unwrap().1;
                for (name, r) in category.as_dict().unwrap().iter() {
                    if name.starts_with(b"Sh") {
                        reached.insert((kind.to_vec(), r.as_reference().unwrap()));
                    }
                }
            }
        }
        let count = |kind: &[u8]| reached.iter().filter(|(k, _)| k == kind).count();
        // Text in one face, one opacity, a list for each z.
        let counts = (count(b"Font"), count(b"ExtGState"), count(b"Properties"));
        assert_eq!(counts, (1, 1, 2), "{cycle}");
    }
}

/// A page whose block draws nothing is still one the generator is on, so
/// its pages are found as they were: the block another page draws alike
/// emptied by hand, the watermark is on both pages again, and drawn on
/// both by the next save.
#[test]
fn an_empty_block_keeps_its_page_in_scope() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let on = [doc.page(0).unwrap(), doc.page(2).unwrap()];
    doc.commit_ops(vec![watermark("DRAFT", Some(&on))], "watermark")
        .unwrap();
    let id = doc.state().watermark(Z::Over).unwrap().id;
    let mut pdf = Pdf::load_mem(&How::Rewritten.write(&doc)).unwrap();
    let third = pages(&pdf)[2];
    let ids = contents(&pdf, third);
    let (mut dict, _) = stream(&pdf, ids[3]);
    let empty = sheaf_util::checksum(&[]).to_be_bytes().to_vec();
    let marker = dict.get_mut(b"SheafBlock").unwrap().as_dict_mut().unwrap();
    marker.set("L", Object::String(empty, lopdf::StringFormat::Hexadecimal));
    dict.remove(b"Filter");
    let block = pdf.add_object(Stream::new(dict, Vec::new()));
    let mut entries: Vec<Object> = ids.iter().map(|&i| Object::Reference(i)).collect();
    entries[3] = Object::Reference(block);
    (pdf.get_object_mut(third).unwrap().as_dict_mut().unwrap()).set("Contents", entries);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let (_, again) = opened(&dir, "b.pdf", &bytes);
    let saved = again.primary().generators();
    assert_eq!(saved[0].pages, Some(vec![0, 2]));
    let on: im::OrdSet<PageId> = [0, 2].map(|i| again.page(i).unwrap()).into_iter().collect();
    let wm = again.state().watermark(Z::Over).unwrap();
    assert_eq!(wm.scope().into_owned(), Scope::Pages(on));
    let pdf = Pdf::load_mem(&How::Rewritten.write(&again)).unwrap();
    assert_eq!(blocks(&pdf), vec![vec![id], vec![], vec![id]]);
    assert!(drawn(&pdf, pages(&pdf)[2]).contains("(DRAFT)"));
}

/// A page brought in from a file sheaf saved with a watermark keeps it,
/// another file's, drawn as that file drew it, isolated from the
/// background drawn under it, which doesn't replace a watermark; removed
/// from that page, it's taken out, and the streams that isolated it with
/// it. Both ways.
fn foreign(how: How) {
    let dir = scratch();
    let (_, mut other) = opened(&dir, "other.pdf", &text_file(1));
    other
        .commit_ops(vec![watermark("THEIRS", None)], "watermark")
        .unwrap();
    let theirs = other.state().watermark(Z::Over).unwrap().id;
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(1));
    let src = super::tests::add(&doc, &How::Rewritten.write(&other), "other.pdf");
    super::tests::insert(&mut doc, &src, 0);
    let background = fx::set_watermark(fx::watermark("MINE"), Z::Under, None);
    doc.commit_ops(vec![background], "background").unwrap();
    let mine = doc.state().watermark(Z::Under).unwrap().id;
    let pdf = Pdf::load_mem(&how.write(&doc)).unwrap();
    let brought = pages(&pdf)[1];
    assert_eq!(
        blocks(&pdf),
        vec![vec![mine], vec![mine, theirs]],
        "{how:?}"
    );
    let ids = contents(&pdf, brought);
    let kinds: Vec<bool> = ids.iter().map(|&c| isolates(&pdf, c)).collect();
    // Ours around theirs: their isolated content and block between ours.
    assert_eq!(
        kinds,
        [false, true, true, false, true, false, true],
        "{how:?}"
    );
    assert_eq!(depth(&pdf, brought).1, Balance::default());
    let text = drawn(&pdf, brought);
    assert!(
        text.contains("(THEIRS)") && text.contains("(MINE)"),
        "{text}"
    );
    let page = doc.page(1).unwrap();
    let c = (doc.commit_ops(vec![fx::remove_watermark(Some(Z::Over), &[page])], "remove")).unwrap();
    let def = doc.state().def(page).unwrap();
    let filter = Filter::DropStreams(StreamSel::SheafGen(theirs));
    assert_eq!(def.content.filters, im::Vector::unit(filter));
    assert_eq!(
        c.lines[0].text,
        "Remove the watermark from page 2 (1 from other files)"
    );
    let pdf = Pdf::load_mem(&how.write(&doc)).unwrap();
    assert_eq!(blocks(&pdf), vec![vec![mine], vec![mine]], "{how:?}");
    let ids = contents(&pdf, pages(&pdf)[1]);
    let kinds: Vec<bool> = ids.iter().map(|&c| isolates(&pdf, c)).collect();
    assert_eq!(
        kinds,
        [false, true, false, true],
        "{how:?}: its own content"
    );
    assert!(!drawn(&pdf, pages(&pdf)[1]).contains("THEIRS"));
}

crate::both_ways!(
    another_file_s_watermark_is_kept_until_removed,
    another_file_s_watermark_is_kept_until_removed_appended: foreign
);

/// A page brought in from a file sheaf saved with a watermark, under a
/// watermark of the document's, draws the document's alone: theirs is of
/// the kind ours replaces, and goes with the streams that isolated it.
/// Both ways.
fn replaced(how: How) {
    let dir = scratch();
    let (_, mut other) = opened(&dir, "other.pdf", &text_file(1));
    other
        .commit_ops(vec![watermark("THEIRS", None)], "watermark")
        .unwrap();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(1));
    let src = super::tests::add(&doc, &How::Rewritten.write(&other), "other.pdf");
    super::tests::insert(&mut doc, &src, 0);
    doc.commit_ops(vec![watermark("MINE", None)], "watermark")
        .unwrap();
    let mine = doc.state().watermark(Z::Over).unwrap().id;
    let pdf = Pdf::load_mem(&how.write(&doc)).unwrap();
    assert_eq!(blocks(&pdf), vec![vec![mine], vec![mine]], "{how:?}");
    let brought = pages(&pdf)[1];
    let ids = contents(&pdf, brought);
    let kinds: Vec<bool> = ids.iter().map(|&c| isolates(&pdf, c)).collect();
    assert_eq!(kinds, [true, false, true, false], "{how:?}");
    assert_eq!(depth(&pdf, brought).1, Balance::default());
    let text = drawn(&pdf, brought);
    assert!(
        !text.contains("THEIRS") && text.matches("(MINE)").count() == 1,
        "{text}"
    );
}

crate::both_ways!(
    another_file_s_watermark_is_replaced_by_ours,
    another_file_s_watermark_is_replaced_by_ours_appended: replaced
);

/// A watermark another tool merged into the page's own stream isn't a
/// block the page holds, but its ranges name it: removed, they're cut out
/// of that stream, and the rest of it stays.
#[test]
fn a_merged_watermark_is_cut_out() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(1));
    doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
        .unwrap();
    let id = doc.state().watermark(Z::Over).unwrap().id;
    let mut pdf = Pdf::load_mem(&How::Rewritten.write(&doc)).unwrap();
    let page = pages(&pdf)[0];
    let merged: Vec<u8> = (contents(&pdf, page).into_iter())
        .flat_map(|c| {
            let mut data = stream(&pdf, c).1;
            data.push(b'\n');
            data
        })
        .collect();
    let one = pdf.add_object(Stream::new(dictionary! {}, merged));
    (pdf.get_object_mut(page).unwrap().as_dict_mut().unwrap()).set("Contents", one);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let (_, mut again) = opened(&dir, "b.pdf", &bytes);
    // Its catalog says what it is, but no page holds a block of it.
    assert_eq!(again.state().watermark(Z::Over).unwrap().id, id);
    let all: Vec<PageId> = again.state().order().iter().copied().collect();
    again
        .commit_ops(vec![fx::remove_watermark(None, &all)], "remove")
        .unwrap();
    let filter = Filter::DropMarked(MarkedSel::SheafGen(id));
    let def = again.state().def(all[0]).unwrap();
    assert_eq!(def.content.filters, im::Vector::unit(filter));
    let pdf = Pdf::load_mem(&How::Rewritten.write(&again)).unwrap();
    let text = drawn(&pdf, pages(&pdf)[0]);
    assert!(
        !text.contains("DRAFT") && text.contains("(page 0)"),
        "{text}"
    );
    assert_eq!(depth(&pdf, pages(&pdf)[0]).1, Balance::default());
}

/// Three blank pages share one stream, which holds another file's
/// watermark merged into it: taken off the first page, that page draws a
/// copy of the stream without it, and the others the stream as it was.
/// Both ways.
fn shared(how: How) {
    let dir = scratch();
    let mut pdf = Pdf::with_version("1.7");
    let root = pdf.new_object_id();
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let list = dictionary! {
        "Type" => "Pagination", "Subtype" => "Watermark",
        "SheafGen" => Object::string_literal("g1f"),
    };
    let content = b"/Artifact /P0 BDC q BT /F1 30 Tf 100 100 Td (THEIRS) Tj ET Q EMC\n\
                    BT /F1 24 Tf 72 72 Td (shared) Tj ET";
    let shared = pdf.add_object(Stream::new(dictionary! {}, content.to_vec()));
    let kids: Vec<Object> = (0..3)
        .map(|_| {
            let page = dictionary! {
                "Type" => "Page", "Parent" => root, "Contents" => shared,
            };
            pdf.add_object(page).into()
        })
        .collect();
    pdf.objects.insert(
        root,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => 3,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => font },
                "Properties" => dictionary! { "P0" => list },
            },
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let (_, mut doc) = opened(&dir, "blank.pdf", &bytes);
    let first = doc.page(0).unwrap();
    doc.commit_ops(vec![fx::remove_watermark(None, &[first])], "remove")
        .unwrap();
    let written = Pdf::load_mem(&how.write(&doc)).unwrap();
    let firsts = contents(&written, pages(&written)[0]);
    assert_eq!(firsts.len(), 1);
    assert_ne!(firsts[0], shared, "{how:?}: a copy");
    let text = drawn(&written, pages(&written)[0]);
    assert!(
        !text.contains("THEIRS") && text.contains("(shared)"),
        "{text}"
    );
    for page in &pages(&written)[1..] {
        assert_eq!(contents(&written, *page), [shared], "{how:?}");
    }
    assert_eq!(stream(&written, shared).1, content.to_vec(), "{how:?}");
}

crate::both_ways!(
    a_stream_pages_share_is_copied_where_filtered,
    a_stream_pages_share_is_copied_where_filtered_appended: shared
);

/// A file saved with a watermark and opened again appends what changes,
/// and none of the watermark it doesn't: a comment, its page alone; a page
/// turned, its page and a block for it, its resources and the streams that
/// isolate it as they were; the watermark moved, its pages, its block and
/// what the generators are. The removal check finds nothing of what's set
/// aside.
#[test]
fn a_found_watermark_appends_only_what_changes() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
        .unwrap();
    let (path, mut again) = reopened(&dir, "b.pdf", &doc, How::Rewritten);
    let p0 = again.page(0).unwrap();
    let appended = |again: &mut Document, ops: Vec<Op>| {
        again.commit_ops(ops, "edit").unwrap();
        let (base, section) = planned_in_place(again);
        keys_at(again, &written(base.len(), &section, &path))
    };
    let keys = appended(&mut again, vec![fx::add(p0, &square(1))]);
    let page = |k: &ObjKey| matches!(k, ObjKey::Page(_));
    assert_eq!(keys.iter().filter(|k| page(k)).count(), 1);
    let drawn = |k: &ObjKey| {
        matches!(
            k,
            ObjKey::Block(_) | ObjKey::Shared(_) | ObjKey::PageRes { .. } | ObjKey::Generators
        )
    };
    assert!(!keys.iter().any(drawn), "{keys:?}");
    let keys = appended(&mut again, vec![fx::rotate(&[p0], Turn::Quarter)]);
    assert_eq!(kinds(&keys), BTreeMap::from([("page", 1), ("block", 1)]));
    let mut moved = fx::watermark("DRAFT");
    moved.position = GridPos::Top;
    let keys = appended(&mut again, vec![fx::set_watermark(moved, Z::Over, None)]);
    let want = BTreeMap::from([("page", 3), ("block", 2), ("generators", 1)]);
    assert_eq!(kinds(&keys), want);
    assert_eq!(super::removal::vetoes_here().derived, 0);
}

/// What `/SheafGenerators` says that sheaf wouldn't write is passed over:
/// another version of it, a second generator at a z, one on some pages
/// none of which holds a block of it, one that can't be read, the others
/// still found, one that replaces more than its own kind, and one that
/// can't be drawn.
#[test]
fn what_sheaf_wouldn_t_write_isn_t_found() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    let first = doc.page(0).unwrap();
    let under = fx::set_watermark(fx::watermark("under"), Z::Under, Some(&[first]));
    doc.commit_ops(vec![watermark("DRAFT", None), under], "watermark")
        .unwrap();
    let bytes = How::Rewritten.write(&doc);
    let found = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut pdf = Pdf::load_mem(&bytes).unwrap();
        let catalog = pdf.catalog().unwrap();
        let at = catalog
            .get(b"SheafGenerators")
            .unwrap()
            .as_reference()
            .unwrap();
        let mut json: serde_json::Value =
            serde_json::from_slice(pdf.get_object(at).unwrap().as_str().unwrap()).unwrap();
        edit(&mut json);
        let json = serde_json::to_vec(&json).unwrap();
        pdf.objects.insert(at, Object::string_literal(json));
        let mut out = Vec::new();
        pdf.save_to(&mut out).unwrap();
        let (_, again) = opened(&dir, "b.pdf", &out);
        let mut zs: Vec<Z> = (again.state().generators().values())
            .filter_map(|g| Some(g.spec.watermark()?.z))
            .collect();
        zs.sort();
        zs
    };
    assert_eq!(found(&|_| {}), [Z::Under, Z::Over]);
    assert!(found(&|json| json["v"] = 2.into()).is_empty());
    let second = |json: &mut serde_json::Value| {
        let gens = json["generators"].as_array_mut().unwrap();
        let mut again = gens[0].clone();
        again["id"] = "g1".into();
        again["scope"] = "all".into();
        gens.push(again);
    };
    assert_eq!(found(&second), [Z::Under, Z::Over]);
    let elsewhere = |json: &mut serde_json::Value| {
        for g in json["generators"].as_array_mut().unwrap() {
            if g["scope"] == "pages" {
                g["id"] = "g2".into();
            }
        }
    };
    assert_eq!(found(&elsewhere), [Z::Over]);
    let unread = |json: &mut serde_json::Value| {
        let gens = json["generators"].as_array_mut().unwrap();
        gens.push(serde_json::json!({"id": "g3", "kind": "header"}));
    };
    assert_eq!(found(&unread), [Z::Under, Z::Over]);
    let replaces = |json: &mut serde_json::Value| {
        for g in json["generators"].as_array_mut().unwrap() {
            if g["scope"] == "all" {
                g["replaces"] = serde_json::json!(["header", "footer", "watermark"]);
            }
        }
    };
    assert_eq!(found(&replaces), [Z::Under]);
    let unfit = |json: &mut serde_json::Value| {
        for g in json["generators"].as_array_mut().unwrap() {
            g["spec"]["content"]["opacity"] = 2.into();
        }
    };
    assert!(found(&unfit).is_empty());
}

/// `bytes` with a page another tool added at the end, drawing `text`,
/// which holds no block of sheaf's.
fn with_a_page_added(bytes: &[u8], text: &str) -> Vec<u8> {
    let mut pdf = Pdf::load_mem(bytes).unwrap();
    let root = (pdf.catalog().unwrap().get(b"Pages"))
        .and_then(Object::as_reference)
        .unwrap();
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let drawn = format!("BT /F1 24 Tf 72 72 Td ({text}) Tj ET");
    let content = pdf.add_object(Stream::new(dictionary! {}, drawn.into_bytes()));
    let page = pdf.add_object(dictionary! {
        "Type" => "Page", "Parent" => root, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
    });
    let tree = pdf.get_object_mut(root).unwrap().as_dict_mut().unwrap();
    let count = tree.get(b"Count").and_then(Object::as_i64).unwrap();
    tree.set("Count", count + 1);
    let kids = tree
        .get_mut(b"Kids")
        .and_then(Object::as_array_mut)
        .unwrap();
    kids.push(Object::Reference(page));
    let mut out = Vec::new();
    pdf.save_to(&mut out).unwrap();
    out
}

/// A watermark saved on every page, in a file another tool added a page
/// to, is on the pages that hold a block of it: the page added draws none,
/// on screen and as either save writes it.
#[test]
fn a_page_another_tool_added_has_no_watermark() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
        .unwrap();
    let id = doc.state().watermark(Z::Over).unwrap().id;
    let bytes = with_a_page_added(&How::Rewritten.write(&doc), "added");
    let (_, again) = opened(&dir, "b.pdf", &bytes);
    let on: im::OrdSet<PageId> = [0, 1].map(|i| again.page(i).unwrap()).into_iter().collect();
    let wm = again.state().watermark(Z::Over).unwrap();
    assert_eq!((wm.id, wm.scope().into_owned()), (id, Scope::Pages(on)));
    let added = again.page(2).unwrap();
    let st = again.state();
    assert!(crate::realize::untouched(st, added, &ViewInputs::screen()));
    for how in [How::Rewritten, How::Appended] {
        let pdf = Pdf::load_mem(&how.write(&again)).unwrap();
        assert_eq!(blocks(&pdf), [vec![id], vec![id], vec![]], "{how:?}");
        let text = drawn(&pdf, pages(&pdf)[2]);
        assert!(
            !text.contains("DRAFT") && text.contains("(added)"),
            "{text}"
        );
    }
}

/// A watermark on some pages, one of whose blocks another tool merged into
/// the page's own stream, is found on that page too, by the list its
/// resources name: changed, the page draws the new one alone. Both ways.
fn merged_on_some(how: How) {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let on = [doc.page(0).unwrap(), doc.page(1).unwrap()];
    doc.commit_ops(vec![watermark("DRAFT", Some(&on))], "watermark")
        .unwrap();
    let id = doc.state().watermark(Z::Over).unwrap().id;
    let mut pdf = Pdf::load_mem(&How::Rewritten.write(&doc)).unwrap();
    let page = pages(&pdf)[1];
    let merged: Vec<u8> = (contents(&pdf, page).into_iter())
        .flat_map(|c| {
            let mut data = stream(&pdf, c).1;
            data.push(b'\n');
            data
        })
        .collect();
    let one = pdf.add_object(Stream::new(dictionary! {}, merged));
    (pdf.get_object_mut(page).unwrap().as_dict_mut().unwrap()).set("Contents", one);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let (_, mut again) = opened(&dir, "b.pdf", &bytes);
    assert_eq!(again.primary().generators()[0].pages, Some(vec![0, 1]));
    let on: im::OrdSet<PageId> = [0, 1].map(|i| again.page(i).unwrap()).into_iter().collect();
    let wm = again.state().watermark(Z::Over).unwrap();
    assert_eq!(
        (wm.id, wm.scope().into_owned()),
        (id, Scope::Pages(on.clone()))
    );
    let on: Vec<PageId> = on.into_iter().collect();
    again
        .commit_ops(vec![watermark("FINAL", Some(&on))], "watermark")
        .unwrap();
    let pdf = Pdf::load_mem(&how.write(&again)).unwrap();
    assert_eq!(blocks(&pdf), [vec![id], vec![id], vec![]], "{how:?}");
    for page in &pages(&pdf)[..2] {
        let text = drawn(&pdf, *page);
        assert!(
            !text.contains("DRAFT") && text.matches("(FINAL)").count() == 1,
            "{how:?}: {text}"
        );
        assert_eq!(depth(&pdf, *page).1, Balance::default());
    }
}

crate::both_ways!(
    a_merged_block_keeps_its_page_in_scope,
    a_merged_block_keeps_its_page_in_scope_appended: merged_on_some
);

/// A page brought in from an earlier save of the document's file keeps
/// the background that file drew on it, though the document's background,
/// the same generator, isn't on the page: its block is another file's.
/// Both ways.
fn sibling(how: How) {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    let first = doc.page(0).unwrap();
    let under = fx::set_watermark(fx::watermark("MINE"), Z::Under, Some(&[first]));
    doc.commit_ops(vec![under], "background").unwrap();
    let id = doc.state().watermark(Z::Under).unwrap().id;
    let earlier = How::Rewritten.write(&doc);
    let (_, mut later) = opened(&dir, "b.pdf", &earlier);
    let p1 = later.page(1).unwrap();
    later
        .commit_ops(vec![fx::add(p1, &square(1))], "comment")
        .unwrap();
    let (_, mut again) = reopened(&dir, "c.pdf", &later, How::Rewritten);
    assert_eq!(again.state().watermark(Z::Under).unwrap().id, id);
    let src = super::tests::add(&again, &earlier, "earlier.pdf");
    super::tests::insert(&mut again, &src, 0);
    let brought = again.page(2).unwrap();
    assert!(!again.state().watermark(Z::Under).unwrap().covers(brought));
    let pdf = Pdf::load_mem(&how.write(&again)).unwrap();
    assert_eq!(blocks(&pdf), [vec![id], vec![], vec![id]], "{how:?}");
    let text = drawn(&pdf, pages(&pdf)[2]);
    assert!(
        text.contains("(MINE)") && text.contains("(page 0)"),
        "{text}"
    );
    assert_eq!(depth(&pdf, pages(&pdf)[2]).1, Balance::default());
}

crate::both_ways!(
    a_page_from_an_earlier_save_keeps_its_background,
    a_page_from_an_earlier_save_keeps_its_background_appended: sibling
);

/// A watermark found again and removed from every page is gone from what
/// is made without saving too: a clean copy, every page extracted, and
/// every page printed without markup hold no block of it, and nowhere its
/// text.
#[test]
fn a_found_watermark_removed_is_gone_from_copies() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
        .unwrap();
    let (_, mut again) = reopened(&dir, "b.pdf", &doc, How::Rewritten);
    let all: Vec<PageId> = again.state().order().iter().copied().collect();
    again
        .commit_ops(vec![fx::remove_watermark(None, &all)], "remove")
        .unwrap();
    let snapshot = again.snapshot();
    let out = dir.join("clean.pdf");
    crate::save::export_clean_copy(&snapshot, &out).unwrap();
    let made = [
        ("clean copy", std::fs::read(&out).unwrap()),
        (
            "extracted",
            crate::save::extract_bytes(&snapshot, &all).unwrap(),
        ),
        (
            "printed",
            crate::save::print_pdf(&snapshot, &all, false).unwrap(),
        ),
    ];
    for (what, bytes) in made {
        let pdf = Pdf::load_mem(&bytes).unwrap();
        assert_eq!(blocks(&pdf), vec![Vec::<GenId>::new(); 2], "{what}");
        let all = everything(&pdf);
        for gone in ["DRAFT", "SheafBlock"] {
            assert!(!all.contains(gone), "{what}: {gone}");
        }
    }
}

/// A file sheaf saved with a watermark whose catalog another tool wrote
/// without `/SheafGenerators` holds sheaf's blocks all the same: watermarked
/// again, each page's old block and the streams that isolated its content
/// are left out whole, so it holds one block, the new one, isolated once,
/// and no copy of a stream. Both ways.
fn uncataloged(how: How) {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
        .unwrap();
    let mut pdf = Pdf::load_mem(&How::Rewritten.write(&doc)).unwrap();
    let catalog = pdf.trailer.get(b"Root").and_then(Object::as_reference);
    let catalog = pdf.get_object_mut(catalog.unwrap()).unwrap();
    catalog.as_dict_mut().unwrap().remove(b"SheafGenerators");
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let (_, mut again) = opened(&dir, "b.pdf", &bytes);
    assert!(again.state().generators().is_empty());
    again
        .commit_ops(vec![watermark("FINAL", None)], "watermark")
        .unwrap();
    let id = again.state().watermark(Z::Over).unwrap().id;
    let pdf = Pdf::load_mem(&how.write(&again)).unwrap();
    assert_eq!(blocks(&pdf), vec![vec![id]; 2], "{how:?}");
    for page in pages(&pdf) {
        let ids = contents(&pdf, page);
        let isolating = ids.iter().filter(|&&c| isolates(&pdf, c)).count();
        assert_eq!((ids.len(), isolating), (4, 2), "{how:?}");
        let (deepest, balance) = depth(&pdf, page);
        assert_eq!((deepest, balance), (1, Balance::default()), "{how:?}");
        let text = drawn(&pdf, page);
        assert!(
            !text.contains("DRAFT") && text.contains("(FINAL)"),
            "{text}"
        );
    }
}

crate::both_ways!(
    blocks_a_catalog_doesn_t_name_are_left_out_whole,
    blocks_a_catalog_doesn_t_name_are_left_out_whole_appended: uncataloged
);

/// A stream sheaf wrote to isolate a page's content, which another tool
/// made draw text too, is the page's own content: the watermark moved,
/// or taken off, the page still draws it. Both ways.
fn drawing_isolation(how: How) {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(1));
    doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
        .unwrap();
    let mut pdf = Pdf::load_mem(&How::Rewritten.write(&doc)).unwrap();
    let open = contents(&pdf, pages(&pdf)[0])[0];
    assert!(isolates(&pdf, open));
    let mut stream = pdf.get_object(open).unwrap().as_stream().unwrap().clone();
    stream.dict.remove(b"Filter");
    stream.set_content(b"q BT /F1 12 Tf 72 300 Td (Secret) Tj ET".to_vec());
    pdf.objects.insert(open, stream.into());
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let mut moved = fx::watermark("DRAFT");
    moved.position = GridPos::Top;
    let all = |doc: &Document| doc.state().order().iter().copied().collect::<Vec<_>>();
    let edits: [&dyn Fn(&Document) -> Op; 2] = [
        &|_| fx::set_watermark(moved.clone(), Z::Over, None),
        &|doc| fx::remove_watermark(None, &all(doc)),
    ];
    for edit in edits {
        let (_, mut again) = opened(&dir, "b.pdf", &bytes);
        let op = edit(&again);
        again.commit_ops(vec![op], "edit").unwrap();
        let pdf = Pdf::load_mem(&how.write(&again)).unwrap();
        let text = drawn(&pdf, pages(&pdf)[0]);
        assert!(
            text.contains("(Secret)") && text.contains("(page 0)"),
            "{how:?}: {text}"
        );
    }
}

crate::both_ways!(
    an_isolating_stream_that_draws_is_the_page_s,
    an_isolating_stream_that_draws_is_the_page_s_appended: drawing_isolation
);
