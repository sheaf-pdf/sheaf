//! Exercises the host: each command tests one behavior.

use sheaf_plugin_api::assets::AssetKind;
use sheaf_plugin_api::sheaf_edit::{
    AnnotId, AnnotRef, BlobId, BlobSel, BookmarkPatch, BookmarkRef, Box2, Bring, FlattenSet, Local,
    Made, MarkKind, MarkPatch, NewMark, Op, OutlineId, PageSel, PageSource, Patch, Pt,
};
use sheaf_plugin_api::types::{AnnotKind, AnnotView, DestView, DocEvent, PageInfo};
use sheaf_plugin_api::ui::{self, Field, FieldKind, FieldValue, Form, PanelRow, RowAction};
use sheaf_plugin_api::{Applied, Guest, Ops, document, edits, log, ops, storage, text, view};

static KEPT: std::sync::Mutex<Option<sheaf_plugin_api::render::Image>> =
    std::sync::Mutex::new(None);

struct TestPlugin;

/// `ops` submitted as `label`, its failure as the call's error.
fn submit(ops: Ops, label: &str) -> Result<(), String> {
    ops.submit(label).map(|_| ()).map_err(|f| f.message)
}

/// Page 1.
fn first() -> Result<PageInfo, String> {
    document::pages()
        .into_iter()
        .next()
        .ok_or("no pages".into())
}

/// "x,y".
fn point(args: Option<String>) -> Result<Pt, String> {
    let arg = args.unwrap_or_default();
    let (x, y) = arg.split_once(',').ok_or("expected x,y")?;
    let parse = |v: &str| v.parse::<f32>().map_err(|e| e.to_string());
    Ok(Pt {
        x: parse(x)?,
        y: parse(y)?,
    })
}

/// A box as "x,y wxh".
fn shown(b: Box2) -> String {
    format!("{},{} {}x{}", b.x, b.y, b.w, b.h)
}

/// What a submit made, by id: "5 6".
fn made(a: &Applied) -> String {
    let ids = a.created.iter().flatten().map(|m| match m {
        Made::Page(p) => p.0,
        Made::Bookmark(b) => b.0,
        Made::Annot(n) => n.0,
    });
    ids.map(|n| n.to_string()).collect::<Vec<_>>().join(" ")
}

/// `envelope` submitted as `label`, and sheaf's answer told back through
/// a notice as it said it, JSON: the call goes on either way.
fn relay(label: &str, envelope: &str) {
    let (Ok(answer) | Err(answer)) = edits::submit(label, envelope);
    ui::notice(&answer);
}

/// A mark of `kind` on `p` saying `contents`, in blue.
fn new_mark(p: &PageInfo, kind: MarkKind, contents: &str) -> NewMark {
    let made = Ops::new().mark(p, kind, ops::style(ops::BLUE), contents);
    match made.ops() {
        [Op::AddMark { mark, .. }] => mark.clone(),
        other => unreachable!("{other:?}"),
    }
}

/// A form with nothing to fill in, which the user answers once they're
/// done with whatever they do meanwhile.
fn wait_for_the_user() {
    ui::show_form(&Form {
        title: "Wait".into(),
        fields: Vec::new(),
        submit_label: None,
    });
}

/// A 2×2 red PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0xfd, 0xd4, 0x9a,
    0x73, 0x00, 0x00, 0x00, 0x10, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xb8, 0xa0, 0xa0, 0x00,
    0x44, 0x0c, 0x10, 0x0a, 0x00, 0x20, 0x8e, 0x04, 0x41, 0xb9, 0xc8, 0x05, 0x06, 0x00, 0x00, 0x00,
    0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// A one-page PDF of nothing.
fn blank_pdf() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>",
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut at = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        at.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let (xref, size) = (out.len(), objects.len() + 1);
    out.extend(format!("xref\n0 {size}\n0000000000 65535 f \n").bytes());
    for a in at {
        out.extend(format!("{a:010} 00000 n \n").bytes());
    }
    let trailer = format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n");
    out.extend(trailer.bytes());
    out
}

impl Guest for TestPlugin {
    fn activate() -> Result<(), String> {
        log::log(log::Level::Info, "activated");
        Ok(())
    }

    fn run_command(id: String, args_json: Option<String>) -> Result<(), String> {
        match id.as_str() {
            // One bookmark per page, titled with the page's first line.
            "bookmark" => {
                let mut ops = Ops::new();
                for p in document::pages() {
                    let t = text::page_text(p.id)?;
                    let title = t.lines().next().unwrap_or("").trim().to_string();
                    ops = ops.bookmark(&title, Some(p.id));
                }
                submit(ops, "Bookmark pages")
            }
            // Counts to 400 with progress, a millisecond a step, stopping
            // when cancelled: "Stopped at <n>".
            "count" => {
                let task = sheaf_plugin_api::progress::items("Counting", 400);
                let half = task.child("first half", 0.5);
                half.set_total(Some(200), sheaf_plugin_api::progress::Unit::Items);
                for i in 0..400u64 {
                    if task.is_cancelled() {
                        return Err(format!("Stopped at {i}"));
                    }
                    if i < 200 {
                        half.set(i + 1);
                    }
                    task.set(i + 1);
                    if i == 10 {
                        task.message(sheaf_plugin_api::progress::Level::Info, "ten done");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Ok(())
            }
            // Counts 3 of 10 and returns, leaving its task for the host
            // to free.
            "early" => {
                let task = sheaf_plugin_api::progress::items("Early", 10);
                task.set(3);
                std::mem::forget(task);
                Ok(())
            }
            // Holds 300 steps, past the host's cap on live tasks, and a
            // second task past it with a message, says "made", then waits
            // on its last step to be stopped: "Stopped".
            "many-steps" => {
                let task = sheaf_plugin_api::progress::items("Many", 1);
                let steps: Vec<_> = (0..300)
                    .map(|i| task.child(&format!("step {i}"), 0.002))
                    .collect();
                let late = sheaf_plugin_api::progress::items("Late", 1);
                late.message(sheaf_plugin_api::progress::Level::Info, "hidden");
                task.message(sheaf_plugin_api::progress::Level::Info, "made");
                loop {
                    sheaf_plugin_api::progress::check(&steps[299])?;
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
            // Two submits, a bookmark each, into the call's one
            // transaction.
            "two-submits" => {
                for title in ["one", "two"] {
                    submit(Ops::new().bookmark(title, None), title)?;
                }
                Ok(())
            }
            "context" => Err(format!(
                "path={:?} version={:?} selection={:?} current={:?} args={:?}",
                document::path(),
                document::version().map(|v| v.id),
                document::selection(),
                document::current_page(),
                args_json
            )),
            // Ask for a title, then bookmark the first page picked with it.
            "form" => {
                let form = Form {
                    title: "Bookmark".into(),
                    fields: vec![
                        Field {
                            id: "title".into(),
                            label: "Title".into(),
                            kind: FieldKind::Text(ui::TextField {
                                default: "Untitled".into(),
                                placeholder: String::new(),
                            }),
                        },
                        Field {
                            id: "pages".into(),
                            label: "Pages".into(),
                            kind: FieldKind::PageRange(String::new()),
                        },
                    ],
                    submit_label: None,
                };
                let Some(answers) = ui::show_form(&form) else {
                    return Err("cancelled".into());
                };
                let mut title = String::new();
                let mut target = None;
                for a in answers {
                    match (a.id.as_str(), a.value) {
                        ("title", FieldValue::Text(t)) => title = t,
                        ("pages", FieldValue::PageRange(p)) => target = p.first().copied(),
                        _ => {}
                    }
                }
                submit(Ops::new().bookmark(&title, target), "Bookmark")
            }
            // One row per match of the argument regex on every page, with
            // where it shows.
            "panel" => {
                let re = args_json.unwrap_or_else(|| r"\d+".into());
                let mut rows = Vec::new();
                for p in document::pages() {
                    for (i, m) in text::pattern_search(p.id, &re)?.into_iter().enumerate() {
                        let r = view::box_to_view(&p, m.rects[0].into());
                        rows.push(PanelRow {
                            id: format!("{}-{i}", p.id),
                            label: m.text,
                            detail: format!("{:.0},{:.0}", r.x, r.y),
                            badge: None,
                            icon: ui::Icon::Page,
                            depth: 0,
                            page: Some(p.id),
                            actions: vec![RowAction {
                                id: "pick".into(),
                                label: "Pick".into(),
                            }],
                        });
                    }
                }
                ui::set_panel("matches", &rows);
                ui::set_panel("undeclared", &rows);
                ui::show_panel("matches");
                ui::show_panel("comments");
                ui::show_panel("nowhere");
                ui::notice(&format!("{} matches", rows.len()));
                Ok(())
            }
            // Storage: "k=v" sets, "k" reads (the answer comes back as the error).
            "store" => {
                let arg = args_json.unwrap_or_default();
                match arg.split_once('=') {
                    Some((k, v)) => storage::set(k, v),
                    None => Err(format!("{:?}", storage::get(&arg))),
                }
            }
            "settings" => Err(storage::settings()),
            "prefs" => Err(storage::preferences()),
            // The kinds on page 1 ("polygon:dashed,measure") into storage
            // as "kinds", and a filled, dashed polygon whose text names the
            // user.
            "marks" => {
                let p = first()?;
                let kinds: Vec<String> = document::annotations(p.id)
                    .iter()
                    .map(|v| match v {
                        AnnotView::Mark(a) => {
                            let k = match &a.kind {
                                AnnotKind::Polygon(_) => "polygon",
                                AnnotKind::Measure(_) => "measure",
                                AnnotKind::Highlight(_) => "highlight",
                                _ => "other",
                            };
                            match a.dashed {
                                true => format!("{k}:dashed"),
                                false => k.to_string(),
                            }
                        }
                        AnnotView::Comment(c) => format!("comment:{}", c.subtype),
                    })
                    .collect();
                let points = vec![
                    Pt { x: 10.0, y: 10.0 },
                    Pt { x: 60.0, y: 10.0 },
                    Pt { x: 60.0, y: 60.0 },
                ];
                let mut style = ops::style(ops::BLUE);
                style.fill = Some([255, 0, 0]);
                style.dashed = true;
                let kind = MarkKind::Polygon { points };
                let poly = Ops::new().mark(&p, kind, style, "Checked by <<initials>>");
                submit(poly, "Add polygon")?;
                storage::set("kinds", &kinds.join(","))
            }
            "events" => Err(storage::get("events").unwrap_or_default()),
            // Each page's turn, the size it shows and its label:
            // "90 740x580 1; 0 612x792 2".
            "pages" => {
                use sheaf_plugin_api::types::Rotation;
                let pages: Vec<String> = document::pages()
                    .iter()
                    .map(|p| {
                        let degrees = match p.rotate {
                            Rotation::R0 => 0,
                            Rotation::R90 => 90,
                            Rotation::R180 => 180,
                            Rotation::R270 => 270,
                        };
                        let (w, h) = view::shown_size(p);
                        format!("{degrees} {w}x{h} {}", p.label)
                    })
                    .collect();
                Err(pages.join("; "))
            }
            // At "x,y" on page 1 as it shows: a 40×20 square, and a
            // bookmark landing there.
            "place" => {
                let p = first()?;
                let shown = point(args_json)?;
                let rect = Box2 {
                    x: shown.x,
                    y: shown.y,
                    w: 40.0,
                    h: 20.0,
                };
                let square = MarkKind::Square {
                    rect: view::box_from_view(&p, rect),
                };
                let at = view::from_view(&p, shown);
                let ops = Ops::new()
                    .mark(&p, square, ops::style(ops::BLUE), "")
                    .bookmark_in(None, "both", Some(p.id), Some(ops::at(at.x, at.y)), None);
                submit(ops, "Place")
            }
            // Every bookmark lands at "x,y" as its own page shows.
            "reposition" => {
                let shown = point(args_json)?;
                let pages = document::pages();
                let mut ops = Ops::new();
                for o in document::outline() {
                    let Some(p) = pages.iter().find(|p| Some(p.id) == o.target) else {
                        continue;
                    };
                    let at = view::from_view(p, shown);
                    ops = ops.push(Op::PatchBookmarks {
                        bookmarks: vec![BookmarkRef::Id(OutlineId(o.id))],
                        patch: BookmarkPatch {
                            view: Patch::Set(ops::at(at.x, at.y)),
                            ..BookmarkPatch::default()
                        },
                    });
                }
                submit(ops, "Reposition")
            }
            // Page 1's squares as it shows them, and where each bookmark
            // lands, in PageSpace: "square 100,200 40x20; both
            // Some(220.0),Some(130.0)".
            "where" => {
                let p = first()?;
                let mut found: Vec<String> = document::annotations(p.id)
                    .iter()
                    .filter_map(|v| match v {
                        AnnotView::Mark(a) => match &a.kind {
                            AnnotKind::Square(r) => Some(format!(
                                "square {}",
                                shown(view::box_to_view(&p, (*r).into()))
                            )),
                            _ => None,
                        },
                        AnnotView::Comment(_) => None,
                    })
                    .collect();
                found.extend(document::outline().iter().filter_map(|o| match &o.view {
                    Some(DestView::Xyz(v)) => Some(format!("{} {:?},{:?}", o.title, v.left, v.top)),
                    _ => None,
                }));
                Err(found.join("; "))
            }
            "write-file" => {
                let path = args_json.unwrap_or_default();
                match std::fs::write(&path, "x") {
                    Ok(()) => Err("wrote".into()),
                    Err(e) => Err(format!("denied: {e}")),
                }
            }
            // Page 1's matches of the query, and its words and text boxes
            // in its top 50 points, each with its boxes, as it shows:
            // "hits Form@0,10 4x10; words ...; boxes 0,10 9x10".
            "find" => {
                use sheaf_plugin_api::types::TextMatch;
                let q = args_json.unwrap_or_default();
                let p = first()?;
                let top = Box2 {
                    x: 0.0,
                    y: 0.0,
                    w: 1000.0,
                    h: 50.0,
                };
                let top = view::box_from_view(&p, top).into();
                let boxes = |rs: &[sheaf_plugin_api::types::Box2]| {
                    let each = rs.iter().map(|r| shown(view::box_to_view(&p, (*r).into())));
                    each.collect::<Vec<_>>().join("+")
                };
                let matched = |ms: Vec<TextMatch>| {
                    let each = ms.iter().map(|m| format!("{}@{}", m.text, boxes(&m.rects)));
                    each.collect::<Vec<_>>().join(" ")
                };
                let hits = matched(text::search(p.id, &q, false)?);
                let words = matched(text::words(p.id, Some(top))?);
                let lines = boxes(&text::text_boxes(p.id, top)?);
                Err(format!("hits {hits}; words {words}; boxes {lines}"))
            }
            // OCR page 1; report the words and the engine.
            "ocr" => {
                let p = first()?.id;
                let layer = sheaf_plugin_api::ocr::recognize(
                    p,
                    &sheaf_plugin_api::ocr::OcrOptions {
                        language: Some("deu".into()),
                        dpi: None,
                    },
                )?;
                let words: Vec<String> = layer.words.iter().map(|w| w.text.clone()).collect();
                Err(format!("{} by {}", words.join(" "), layer.engine))
            }
            // Render a corner of page 1, as it shows, in gray and describe
            // it, where its first pixel shows.
            "render" => {
                use sheaf_plugin_api::render::{PageRenderer, PixelFormat, RenderOptions};
                let p = first()?;
                let r = PageRenderer::new(p.id);
                let (w, h) = r.size();
                let corner = Box2 {
                    x: 10.0,
                    y: 20.0,
                    w: 30.0,
                    h: 40.0,
                };
                let img = r.render(RenderOptions {
                    dpi: 72,
                    region: Some(view::box_from_view(&p, corner).into()),
                    format: PixelFormat::Gray8,
                    without_markup: false,
                })?;
                let first = img.read_rect(0, 0, 1, 1);
                let at = view::to_view(&p, img.to_page(0.0, 0.0).into());
                let rows = img.read_rows(0, 2).len();
                let (iw, ih) = (img.width(), img.height());
                // Kept past the call: the host frees it anyway.
                *KEPT.lock().unwrap() = Some(img);
                Err(format!(
                    "{w}x{h} -> {iw}x{ih} first {first:?} at {},{} rows {rows}",
                    at.x, at.y
                ))
            }
            "kept-width" => Err(format!(
                "{}",
                KEPT.lock()
                    .unwrap()
                    .as_ref()
                    .map(|i| i.width())
                    .unwrap_or(99)
            )),
            // Keep rendering small images alive until the host refuses.
            "render-many" => {
                use sheaf_plugin_api::render::{PageRenderer, PixelFormat, RenderOptions};
                let r = PageRenderer::new(first()?.id);
                let mut alive = Vec::new();
                loop {
                    match r.render(RenderOptions {
                        dpi: 10,
                        region: None,
                        format: PixelFormat::Rgba8,
                        without_markup: false,
                    }) {
                        Ok(i) => alive.push(i),
                        Err(e) => return Err(format!("{} then: {e}", alive.len())),
                    }
                }
            }
            // A note and a reply to it, the reply naming the note by its
            // local; then the note's text changed, named by the id the
            // submit answered with. Tells what the first submit made.
            "thread" => {
                let p = first()?;
                let note = Op::AddMark {
                    page: ops::page(p.id),
                    mark: new_mark(
                        &p,
                        MarkKind::Note {
                            pos: Pt { x: 20.0, y: 20.0 },
                        },
                        "Tie out",
                    ),
                    reply_to: None,
                    local: Some(Local(1)),
                    imported: None,
                };
                let reply = Op::AddMark {
                    page: ops::page(p.id),
                    mark: new_mark(&p, MarkKind::Reply {}, "Tied"),
                    reply_to: Some(AnnotRef::New(Local(1))),
                    local: None,
                    imported: None,
                };
                let applied = Ops::new().push(note).push(reply).submit("Thread");
                let applied = applied.map_err(|f| f.message)?;
                let Some(Made::Annot(note)) = applied.locals.get(&Local(1)).copied() else {
                    return Err(format!("{applied:?}"));
                };
                let retold = Op::PatchMarks {
                    marks: vec![AnnotRef::Id(note)],
                    patch: MarkPatch {
                        contents: Some("Tie out to the K-1".into()),
                        ..MarkPatch::default()
                    },
                    imported: None,
                };
                submit(Ops::new().push(retold), "Retell")?;
                ui::notice(&made(&applied));
                Ok(())
            }
            // An envelope that names its own label.
            "labelled" => {
                relay(
                    "Labelled",
                    r#"{"v": "1.0", "label": "Mine", "ops": [{"op": "add-bookmark", "at": "last", "title": "x"}]}"#,
                );
                Ok(())
            }
            // Two bookmarks submitted, then a crash.
            "two-then-trap" => {
                submit(Ops::new().bookmark("one", None), "One")?;
                submit(Ops::new().bookmark("two", None), "Two")?;
                unreachable!("a plugin that crashes")
            }
            // A bookmark committed, telling the version it made, another
            // submitted, then a crash.
            "commit-then-trap" => {
                submit(Ops::new().bookmark("one", None), "One")?;
                let v = ops::commit().map_err(|f| f.message)?;
                ui::notice(&format!("committed {}", v.id));
                submit(Ops::new().bookmark("two", None), "Two")?;
                unreachable!("a plugin that crashes")
            }
            // A square on page 1 committed, then a wait while the user
            // edits, then a crash.
            "commit-wait-trap" => {
                let p = first()?;
                let square = MarkKind::Square {
                    rect: Box2 {
                        x: 10.0,
                        y: 10.0,
                        w: 20.0,
                        h: 20.0,
                    },
                };
                submit(
                    Ops::new().mark(&p, square, ops::style(ops::BLUE), ""),
                    "Square",
                )?;
                ops::commit().map_err(|f| f.message)?;
                wait_for_the_user();
                unreachable!("a plugin that crashes")
            }
            // Page 3's tapes counted, a wait while the user edits, then
            // their count added on page 5: tells what the submit made.
            "subtotal" => {
                let pages = document::pages();
                let (Some(third), Some(fifth)) = (pages.get(2), pages.get(4)) else {
                    return Err("too few pages".into());
                };
                let tapes = (document::annotations(third.id).iter())
                    .filter(
                        |v| matches!(v, AnnotView::Mark(a) if matches!(a.kind, AnnotKind::Tape(_))),
                    )
                    .count();
                wait_for_the_user();
                let total = MarkKind::FreeText {
                    rect: Box2 {
                        x: 50.0,
                        y: 50.0,
                        w: 100.0,
                        h: 20.0,
                    },
                    size: 10.0,
                };
                let ops = Ops::new().mark(
                    fifth,
                    total,
                    ops::style(ops::BLUE),
                    &format!("{tapes} tapes"),
                );
                let applied = ops.submit("Total").map_err(|f| f.message)?;
                ui::notice(&made(&applied));
                Ok(())
            }
            // What a plugin may not do, each told back as sheaf answered:
            // pages inserted from a file it uploads,
            "insert-pages" => {
                let file = ops::upload(&blank_pdf(), AssetKind::File)?;
                let insert = Op::InsertPages {
                    from: PageSource::File {
                        file,
                        name: "blank.pdf".into(),
                    },
                    pages: None,
                    before: None,
                    bring: Bring::default(),
                    locals: Vec::new(),
                };
                relay("Insert", &Ops::new().push(insert).to_json());
                Ok(())
            }
            // page 1's first comment of another tool's deleted,
            "delete-comment" => {
                let p = first()?;
                let comment = (document::annotations(p.id).into_iter()).find_map(|v| match v {
                    AnnotView::Comment(c) => Some(c.id),
                    AnnotView::Mark(_) => None,
                });
                let delete = Op::DeleteAnnotations {
                    annots: vec![AnnotRef::Id(AnnotId(comment.ok_or("no comment")?))],
                };
                relay("Delete", &Ops::new().push(delete).to_json());
                Ok(())
            }
            // every page's annotations flattened,
            "flatten" => {
                let flatten = Op::FlattenAnnotations {
                    pages: PageSel::All,
                    which: FlattenSet::All,
                };
                relay("Flatten", &Ops::new().push(flatten).to_json());
                Ok(())
            }
            // and a page stamp of the blob the argument names, by its id.
            "raw-blob" => {
                let p = first()?;
                let stem = args_json.unwrap_or_default();
                let id = BlobId::from_stem(&stem).ok_or("not a blob's name")?;
                let rect = Box2 {
                    x: 10.0,
                    y: 10.0,
                    w: 50.0,
                    h: 50.0,
                };
                let stamp = Ops::new().page_stamp(&p, rect, BlobSel::Id(id), "Page");
                relay("Stamp", &stamp.to_json());
                Ok(())
            }
            // A note asking for the user's name, then page 1's
            // annotations, by whom and what they say, told back: all it
            // learns.
            "authors" => {
                let p = first()?;
                let note = MarkKind::Note {
                    pos: Pt { x: 30.0, y: 30.0 },
                };
                let ops = Ops::new().mark(&p, note, ops::style(ops::BLUE), "Seen by <<name>>");
                submit(ops, "Note")?;
                let told: Vec<String> = (document::annotations(p.id).iter())
                    .map(|v| match v {
                        AnnotView::Mark(a) => format!("{}: {}", a.author, a.contents),
                        AnnotView::Comment(c) => format!("{}: {}", c.author, c.contents),
                    })
                    .collect();
                ui::notice(&told.join("; "));
                Ok(())
            }
            // A PNG uploaded and stamped on page 1, committed, then
            // stamped again by the same handle.
            "stamp-upload" => {
                let p = first()?;
                let png = ops::upload(PNG, AssetKind::Image)?;
                let rect = Box2 {
                    x: 100.0,
                    y: 100.0,
                    w: 40.0,
                    h: 40.0,
                };
                submit(Ops::new().page_stamp(&p, rect, png.clone(), "Dot"), "Stamp")?;
                ops::commit().map_err(|f| f.message)?;
                let again = Box2 { x: 200.0, ..rect };
                submit(Ops::new().page_stamp(&p, again, png, "Dot"), "Stamp")
            }
            // A square on page 1, then page 1's annotations: tells what it
            // made and what it was shown, by id.
            "read-back" => {
                let p = first()?;
                let square = MarkKind::Square {
                    rect: Box2 {
                        x: 10.0,
                        y: 10.0,
                        w: 20.0,
                        h: 20.0,
                    },
                };
                let ops = Ops::new().mark(&p, square, ops::style(ops::BLUE), "");
                let applied = ops.submit("Square").map_err(|f| f.message)?;
                let listed: Vec<String> = (document::annotations(p.id).iter())
                    .map(|v| match v {
                        AnnotView::Mark(a) => a.id.to_string(),
                        AnnotView::Comment(c) => c.id.to_string(),
                    })
                    .collect();
                ui::notice(&format!(
                    "made {} shown {}",
                    made(&applied),
                    listed.join(" ")
                ));
                Ok(())
            }
            "fail" => Err("nope".into()),
            "trap" => panic!("boom"),
            "spin" => {
                let mut n: u64 = 0;
                loop {
                    n = std::hint::black_box(n.wrapping_add(1));
                }
            }
            "alloc" => {
                let v = vec![1u8; 1 << 30];
                Err(format!("allocated {}", v.len()))
            }
            "read-file" => {
                let path = args_json.unwrap_or_default();
                match std::fs::read(&path) {
                    Ok(b) => Err(format!("read {} bytes", b.len())),
                    Err(e) => Err(format!("denied: {e}")),
                }
            }
            other => Err(format!("unknown command {other}")),
        }
    }

    fn panel_action(panel: String, row: String, action: String) -> Result<(), String> {
        Err(format!("{panel}/{row}/{action}"))
    }

    /// Append each event to storage, for the test to read back; then
    /// answer a change as storage's "answer" says: "mark" adds a square on
    /// page 1, "commit" commits it at once, and "insist" adds it again when
    /// told it was refused.
    fn on_event(event: DocEvent) -> Result<(), String> {
        let changed = matches!(event, DocEvent::Changed(_));
        let refused = matches!(event, DocEvent::EditRefused(_));
        let name = match event {
            DocEvent::Opened => "opened".to_string(),
            DocEvent::Changed(c) => {
                let annots = c.scopes.annots.len();
                format!("changed:{}>{} annots {annots}", c.parent.id, c.version.id)
            }
            DocEvent::Saved(p) => format!("saved:{p}"),
            DocEvent::SelectionChanged => "selection".to_string(),
            DocEvent::Closed => "closed".to_string(),
            DocEvent::Region(r) => {
                let b = r.rect;
                format!("region:{}@{},{} {}x{}", r.page, b.x, b.y, b.w, b.h)
            }
            DocEvent::RegionCleared => "cleared".to_string(),
            DocEvent::EditRefused(why) => format!("refused:{why}"),
        };
        let mut all = storage::get("events").unwrap_or_default();
        if !all.is_empty() {
            all.push(',');
        }
        all.push_str(&name);
        storage::set("events", &all)?;
        let commit = match storage::get("answer").as_deref() {
            Some("mark" | "insist") if changed => false,
            Some("insist") if refused => false,
            Some("commit") if changed => true,
            _ => return Ok(()),
        };
        let p = first()?;
        let square = MarkKind::Square {
            rect: Box2 {
                x: 10.0,
                y: 10.0,
                w: 20.0,
                h: 20.0,
            },
        };
        let ops = Ops::new().mark(&p, square, ops::style(ops::BLUE), "");
        if ops.submit("Answer").is_ok() && commit {
            ops::commit().map_err(|f| f.message)?;
        }
        Ok(())
    }
}

sheaf_plugin_api::export!(TestPlugin);
