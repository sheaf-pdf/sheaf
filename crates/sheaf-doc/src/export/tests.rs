//! Exports: the presets as the app and the command line make them (the
//! old save.rs's extract, print, clean copy and flattened copy tests,
//! ported), what an export keeps of what it leaves out, its gates and
//! asks, and that one stopped writes nothing.

use std::path::Path;

use in_progress::NoProgress;
use lopdf::{Document as Pdf, Object, StringFormat, dictionary};
use sheaf_edit::{OutlinePolicy, PageNumbering, Rotation, Turn};

use super::*;
use crate::annot::{Annot, AnnotId, AnnotKind, Box2};
use crate::forms::fixture::Form;
use crate::history::FixedCount;
use crate::materialize::ocr_tests::{HELLO_HEX, fonts_of, is_glyphless, set_layer};
use crate::materialize::rewrite_tests::{annots_read, make_pdf, open, read_outline, tempdir};
use crate::open::FlatOutlineItem;
use crate::tx::fixture as fx;

/// A mark of `kind`, `color`, by "t".
fn mark(kind: AnnotKind, color: [u8; 3]) -> Annot {
    Annot {
        id: AnnotId(0),
        kind,
        color,
        opacity: 1.0,
        stroke: 2.0,
        author: "t".into(),
        contents: String::new(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    }
}

/// A square at `x`, `y`.
fn square(x: f32, y: f32) -> Annot {
    let rect = Box2 {
        x,
        y,
        w: 80.0,
        h: 40.0,
    };
    mark(AnnotKind::Square { rect }, [0, 120, 255])
}

/// A bookmark titled `title` going to page `page` at `depth`.
fn item(title: &str, page: u32, depth: u16) -> FlatOutlineItem {
    FlatOutlineItem {
        title: title.into(),
        page: Some(page),
        depth,
        view: None,
        color: None,
        style: Default::default(),
        open: true,
        object: None,
        kept: false,
    }
}

/// To the file at `path`, replacing one that's there.
fn to(path: &Path) -> Out {
    Out::File {
        path: path.into(),
        overwrite: true,
    }
}

/// `e` run, answering nothing.
fn run_it(e: Export) -> Result<Exported, ExportError> {
    run(e, &AskAnswers::default(), &NoProgress)
}

/// The subtypes of the annotations `pdf`'s page `page` lists, sorted.
fn subtypes(pdf: &Pdf, page: lopdf::ObjectId) -> Vec<String> {
    let dict = pdf.get_dictionary(page).unwrap();
    let Ok(annots) = dict.get(b"Annots") else {
        return Vec::new();
    };
    let annots = pdf.dereference(annots).unwrap().1.as_array().unwrap();
    let mut out: Vec<String> = (annots.iter())
        .map(|a| {
            let a = pdf.get_dictionary(a.as_reference().unwrap()).unwrap();
            String::from_utf8_lossy(a.get(b"Subtype").unwrap().as_name().unwrap()).into_owned()
        })
        .collect();
    out.sort();
    out
}

/// A clean copy keeps pages, bookmarks, links and form fields, and drops
/// every markup annotation — ours and third-party — along with their
/// now-unreferenced objects. It never replaces the open file.
#[test]
fn clean_copy_strips_markup_only() {
    let dir = tempdir();
    let a_path = make_pdf(&dir, "m.pdf", "M", 2, &[("Top", 0, 0), ("Second", 1, 0)]);
    // Plant third-party annotations on page 1: a note with its popup, a
    // link and a widget outside the form.
    {
        let mut pdf = Pdf::load(&a_path).unwrap();
        let page0 = pdf.get_pages()[&1];
        let rect = |a: f32, b: f32, c: f32, d: f32| {
            Object::Array(vec![a.into(), b.into(), c.into(), d.into()])
        };
        let note = pdf.new_object_id();
        let popup = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Popup", "Rect" => rect(0.0, 0.0, 100.0, 100.0), "Parent" => note,
        });
        pdf.objects.insert(
            note,
            Object::Dictionary(dictionary! {
                "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(10.0, 700.0, 30.0, 720.0),
                "Contents" => Object::String(b"reviewer note".to_vec(), StringFormat::Literal),
                "Popup" => popup,
            }),
        );
        let link = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Link", "Rect" => rect(0.0, 0.0, 50.0, 20.0),
            "Dest" => Object::Array(vec![page0.into(), Object::Name(b"Fit".to_vec())]),
        });
        let field = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx", "T" => Object::String(b"name".to_vec(), StringFormat::Literal),
            "Rect" => rect(100.0, 100.0, 300.0, 120.0),
        });
        pdf.get_dictionary_mut(page0).unwrap().set(
            "Annots",
            Object::Array(vec![note.into(), popup.into(), link.into(), field.into()]),
        );
        pdf.save(&a_path).unwrap();
    }
    let original = std::fs::read(&a_path).unwrap();
    let outline = [item("Top", 0, 0), item("Second", 1, 0)];
    let mut model = open(&a_path, &outline, Box::new(FixedCount(10)));
    let p0 = model.page(0).unwrap();
    let rect = Box2 {
        x: 400.0,
        y: 80.0,
        w: 24.0,
        h: 24.0,
    };
    let tick = AnnotKind::Stamp {
        rect,
        glyph: "✓".into(),
    };
    model
        .commit_ops(vec![fx::add(p0, &mark(tick, [0, 150, 0]))], "tick")
        .unwrap();
    // A connector pair: links in the file, but ours, so they go too.
    let p1 = model.page(1).unwrap();
    let conn = || {
        let rect = Box2 {
            x: 50.0,
            y: 50.0,
            w: 22.0,
            h: 22.0,
        };
        let kind = AnnotKind::Connector {
            rect,
            label: "1".into(),
            partner: None,
        };
        mark(kind, [220, 40, 40])
    };
    model
        .commit_ops(vec![fx::connect((p0, &conn()), (p1, &conn()))], "conn")
        .unwrap();
    // Also rotate page 2 so the copy reflects edits, not just the file.
    model
        .commit_ops(vec![fx::rotate(&[p1], Turn::Quarter)], "rot")
        .unwrap();

    let out = dir.join("clean.pdf");
    let from = || model.export_from(Actor::User);
    run_it(Export::clean_copy(from(), to(&out))).unwrap();
    let pdf = Pdf::load(&out).unwrap();
    let pages = pdf.get_pages();
    assert_eq!(pages.len(), 2);
    assert_eq!(subtypes(&pdf, pages[&1]), ["Link", "Widget"]);
    assert!(!pdf.get_dictionary(pages[&2]).unwrap().has(b"Annots"));
    let rotate = pdf.get_dictionary(pages[&2]).unwrap().get(b"Rotate");
    assert_eq!(rotate.unwrap().as_i64().unwrap(), 90);
    // Nothing markup-shaped survives anywhere in the file.
    for obj in pdf.objects.values() {
        if let Ok(d) = obj.as_dict()
            && let Ok(s) = d.get(b"Subtype").and_then(|s| s.as_name())
        {
            assert!(
                !matches!(s, b"Text" | b"Popup" | b"Stamp"),
                "leftover {}",
                String::from_utf8_lossy(s)
            );
        }
        if let Ok(d) = obj.as_dict() {
            assert!(!d.has(b"SHEF_Annot"));
        }
    }
    let bytes = std::fs::read(&out).unwrap();
    assert!(annots_read(&bytes).sheaf().is_empty());
    assert_eq!(crate::private::found(&pdf), []);
    // Bookmarks kept.
    assert!(pdf.catalog().unwrap().has(b"Outlines"));
    // The open file itself can't be the target, nor a file there without
    // being told to replace it.
    let err = run_it(Export::clean_copy(from(), to(&a_path))).unwrap_err();
    assert!(
        matches!(err, ExportError::Refused(Gate::OwnFile { .. })),
        "{err}"
    );
    assert_eq!(std::fs::read(&a_path).unwrap(), original);
    let kept = Out::File {
        path: out.clone(),
        overwrite: false,
    };
    let err = run_it(Export::clean_copy(from(), kept)).unwrap_err();
    assert!(
        matches!(err, ExportError::Refused(Gate::Exists { .. })),
        "{err}"
    );
    assert_eq!(std::fs::read(&out).unwrap(), bytes);
}

/// Extract keeps the chosen pages in order, their marks, and the
/// bookmarks that point at them (parents of kept children become
/// headings).
#[test]
fn extract_pages_keeps_marks_and_relevant_bookmarks() {
    let dir = tempdir();
    let a = make_pdf(&dir, "x.pdf", "X", 4, &[]);
    let outline = [
        item("Part", 0, 0),
        item("Second", 1, 1),
        item("Fourth", 3, 1),
        item("Third", 2, 0),
    ];
    let mut model = open(&a, &outline, Box::new(FixedCount(10)));
    let ids: Vec<PageId> = (0..4).map(|i| model.page(i).unwrap()).collect();
    let rect = Box2 {
        x: 10.0,
        y: 10.0,
        w: 50.0,
        h: 20.0,
    };
    let note = AnnotKind::Note {
        pos: crate::annot::Pt { x: 5.0, y: 5.0 },
    };
    for (i, kind) in [(1, AnnotKind::Square { rect }), (2, note)] {
        let add = fx::add(ids[i], &mark(kind, [0, 0, 0]));
        model.commit_ops(vec![add], "a").unwrap();
    }
    let out = dir.join("extract.pdf");
    // Pages 2 and 4 (ids given out of order: output keeps document order).
    let from = || model.export_from(Actor::User);
    let made = run_it(Export::extract(from(), &[ids[3], ids[1]], to(&out))).unwrap();
    assert_eq!((made.pages, made.bytes), (2, None));
    let pdf = Pdf::load(&out).unwrap();
    assert_eq!(pdf.get_pages().len(), 2);
    let text = |i: u32| {
        let content = pdf.get_page_content(pdf.get_pages()[&i]);
        String::from_utf8_lossy(&content).into_owned()
    };
    assert!(text(1).contains("MARK X 1"), "{}", text(1));
    assert!(text(2).contains("MARK X 3"), "{}", text(2));
    // Only page 2's square came along.
    let marks = annots_read(&std::fs::read(&out).unwrap()).sheaf();
    assert_eq!(marks.len(), 1);
    assert!(matches!(marks[&0][0].kind, AnnotKind::Square { .. }));
    // Bookmarks: Part (now a heading) > Second, Fourth; Third dropped.
    assert_eq!(
        read_outline(&std::fs::read(&out).unwrap()),
        vec![
            (0, "Part".into(), None),
            (1, "Second".into(), Some(1)),
            (1, "Fourth".into(), Some(2)),
        ]
    );
    let none = run_it(Export::extract(from(), &[], to(&out))).unwrap_err();
    assert!(none.to_string().contains("keeps no page"), "{none}");
}

/// What an export leaves of the bookmarks, each way: as headings (one
/// whose page goes stays only over one that stays), as a delete leaves
/// them, or none. A connector whose other end is left out keeps no
/// partner.
#[test]
fn kept_pages_keep_their_bookmarks_as_the_outline_policy_says() {
    let dir = tempdir();
    let a = make_pdf(&dir, "o.pdf", "O", 3, &[]);
    let outline = [item("One", 0, 0), item("Two", 1, 1), item("Three", 2, 0)];
    let mut model = open(&a, &outline, Box::new(FixedCount(10)));
    let ids: Vec<PageId> = (0..3).map(|i| model.page(i).unwrap()).collect();
    let rect = Box2 {
        x: 50.0,
        y: 50.0,
        w: 22.0,
        h: 22.0,
    };
    let kind = AnnotKind::Connector {
        rect,
        label: "1".into(),
        partner: None,
    };
    let end = mark(kind, [220, 40, 40]);
    model
        .commit_ops(vec![fx::connect((ids[1], &end), (ids[2], &end))], "conn")
        .unwrap();
    let kept = |outline| {
        let keep = Transform::KeepPages {
            pages: PageSel::ids([ids[1]]),
            outline,
            labels: true,
            numbering: PageNumbering::AsKept,
        };
        let mut xtx = ExportTx::new(&model.snapshot(), model.lower_cx(Actor::User));
        xtx.apply(keep, &NoProgress).unwrap();
        xtx.state().clone()
    };
    let titles = |st: &DocState| -> Vec<(u16, String, Option<PageId>)> {
        (st.bookmarks_preorder().into_iter())
            .map(|(depth, _, b)| (depth, b.title.to_string(), b.page()))
            .collect()
    };
    let headings = kept(OutlinePolicy::Headings);
    assert_eq!(
        titles(&headings),
        [(0, "One".into(), None), (1, "Two".into(), Some(ids[1]))]
    );
    let promoted = kept(OutlinePolicy::Promote);
    assert_eq!(titles(&promoted), [(0, "Two".into(), Some(ids[1]))]);
    assert_eq!(titles(&kept(OutlinePolicy::Drop)), []);
    let connector = headings.marks_on(ids[1]).next().unwrap();
    assert!(matches!(
        connector.kind,
        AnnotKind::Connector { partner: None, .. }
    ));
}

#[test]
fn print_flattens_or_drops_markup() {
    let dir = tempdir();
    let a = make_pdf(&dir, "p.pdf", "P", 3, &[]);
    let mut model = open(&a, &[item("Top", 1, 0)], Box::new(FixedCount(10)));
    let ids: Vec<PageId> = (0..3).map(|i| model.page(i).unwrap()).collect();
    let rect = Box2 {
        x: 10.0,
        y: 10.0,
        w: 50.0,
        h: 20.0,
    };
    let red = mark(AnnotKind::Square { rect }, [255, 0, 0]);
    model.commit_ops(vec![fx::add(ids[1], &red)], "a").unwrap();
    let print = |pages: &[PageId], markup| {
        let bytes = printed(model.export_from(Actor::User), pages, markup, &NoProgress);
        Pdf::load_mem(&bytes.unwrap()).unwrap()
    };
    let with = print(&[ids[2], ids[1]], true);
    assert_eq!(with.get_pages().len(), 2);
    let first = with.get_pages()[&1];
    let content = String::from_utf8_lossy(&with.get_page_content(first)).into_owned();
    assert!(content.contains("MARK P 1"), "{content}");
    // The square is drawn into the page, not left as an annotation.
    let annots = with.get_page_annotations(first).unwrap_or_default();
    assert!(annots.is_empty());
    assert!(content.contains(" Do"), "{content}");
    assert!(with.catalog().unwrap().get(b"Outlines").is_err());
    assert!(!with.is_encrypted());
    let without = print(&[ids[1]], false);
    let only = without.get_pages()[&1];
    let annots = without.get_page_annotations(only).unwrap_or_default();
    assert!(annots.is_empty());
    let plain = String::from_utf8_lossy(&without.get_page_content(only)).into_owned();
    assert!(!plain.contains(" Do"), "{plain}");
    let none = Export::print(model.export_from(Actor::User), &[], true);
    assert!(run(none, &PRINT_ANSWERS, &NoProgress).is_err());
}

/// Flattening draws each annotation's appearance into its page's content
/// and removes the annotation, the form's widgets too, which take their
/// fields with them; links stay.
#[test]
fn flattened_copy_burns_marks_into_content() {
    let mut f = Form::new(1);
    let name = f.text("Name", 0, [100.0, 600.0, 300.0, 620.0]);
    f.fill(name, "Ada");
    let link = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link",
        "Rect" => Object::Array(vec![0.into(), 0.into(), 50.into(), 20.into()]),
    });
    let mut annots = f.annots(0);
    annots.push(link);
    f.set_annots(0, &annots);
    let mut model = crate::materialize::tests::open(&f.bytes(), None);
    let p0 = model.page(0).unwrap();
    model
        .commit_ops(vec![fx::add(p0, &square(100.0, 100.0))], "sq")
        .unwrap();
    let e = Export::flattened(model.export_from(Actor::User), Out::Bytes);
    let made = run_it(e).unwrap();
    assert_eq!(made.flattened, 2);
    let bytes = made.bytes.unwrap();
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let page = pdf.get_pages()[&1];
    // Only the link remains as an annotation, and the form has no field.
    assert_eq!(subtypes(&pdf, page), ["Link"]);
    assert!(annots_read(&bytes).sheaf().is_empty());
    let fields = (pdf.catalog().unwrap().get(b"AcroForm").ok())
        .and_then(|f| pdf.dereference(f).ok())
        .and_then(|(_, f)| {
            let fields = f.as_dict().ok()?.get(b"Fields").ok()?;
            fields.as_array().ok().cloned()
        });
    assert_eq!(fields.unwrap_or_default(), []);
    // The content draws a form for each, which the page's resources name:
    // the square's stroke, and the field's value.
    let content = String::from_utf8_lossy(&pdf.get_page_content(page)).into_owned();
    let res = pdf.get_dictionary(page).unwrap().get(b"Resources").unwrap();
    let res = pdf.dereference(res).unwrap().1.as_dict().unwrap();
    let xo = pdf.dereference(res.get(b"XObject").unwrap()).unwrap().1;
    let xo = xo.as_dict().unwrap();
    let drawn = |name: &[u8]| content.contains(&format!("/{} Do", String::from_utf8_lossy(name)));
    let forms: Vec<String> = (xo.iter())
        .filter(|(name, _)| drawn(name))
        .map(|(_, form)| {
            let form = pdf.get_object(form.as_reference().unwrap()).unwrap();
            let form = form.as_stream().unwrap();
            let subtype = form.dict.get(b"Subtype").unwrap().as_name().unwrap();
            assert_eq!(subtype, b"Form");
            String::from_utf8_lossy(&form.decompressed_content().unwrap()).into_owned()
        })
        .collect();
    assert_eq!(forms.len(), 2, "{content}");
    assert!(forms.iter().any(|f| f.contains(" re\nS\n")), "{forms:?}");
    assert!(forms.iter().any(|f| f.contains("(Ada)")), "{forms:?}");
}

#[test]
fn extract_flatten_and_clean_keep_layer() {
    let dir = tempdir();
    let a = make_pdf(&dir, "a.pdf", "A", 2, &[]);
    let mut model = open(&a, &[], Box::new(FixedCount(10)));
    let ids: Vec<PageId> = model.state().order().iter().copied().collect();
    set_layer(&mut model, ids[1], "Hello");
    let red = Annot {
        color: [255, 0, 0],
        ..square(100.0, 100.0)
    };
    model.commit_ops(vec![fx::add(ids[1], &red)], "sq").unwrap();

    let extracted = model.export_bytes(|f, out| Export::extract(f, &[ids[1]], out));
    let pdf = Pdf::load_mem(&extracted).unwrap();
    let page = pdf.get_pages()[&1];
    assert!(String::from_utf8_lossy(&pdf.get_page_content(page)).contains(HELLO_HEX));

    let e = Export::flattened(model.export_from(Actor::User), Out::Bytes);
    let made = run_it(e).unwrap();
    assert_eq!(made.flattened, 1);
    let pdf = Pdf::load_mem(&made.bytes.unwrap()).unwrap();
    let page = pdf.get_pages()[&2];
    let all = String::from_utf8_lossy(&pdf.get_page_content(page)).into_owned();
    assert!(all.contains(HELLO_HEX), "{all}");
    assert!(all.contains(" Do"), "{all}");
    let fonts = fonts_of(&pdf, page);
    assert!(fonts.iter().any(|(_, f)| is_glyphless(&pdf, f)));

    let pdf = Pdf::load_mem(&model.export_bytes(Export::clean_copy)).unwrap();
    let page = pdf.get_pages()[&2];
    assert!(String::from_utf8_lossy(&pdf.get_page_content(page)).contains(HELLO_HEX));
    assert!(!pdf.get_dictionary(page).unwrap().has(b"Annots"));
}

/// A progress cancelled from its `at`th check on, its children's checks
/// counted with its own.
struct CancelAt {
    checks: Arc<std::sync::atomic::AtomicUsize>,
    at: usize,
}

impl Progress for CancelAt {
    fn set_total(&self, _: Option<u64>, _: in_progress::Unit) {}
    fn set(&self, _: u64) {}
    fn inc(&self, _: u64) {}
    fn message(&self, _: in_progress::Level, _: &str) {}
    fn is_cancelled(&self) -> bool {
        let checks = self
            .checks
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        checks + 1 >= self.at
    }
    fn child(&self, _: &str, _: f64) -> Box<dyn Progress> {
        Box::new(CancelAt {
            checks: self.checks.clone(),
            at: self.at,
        })
    }
}

/// A cancelled export writes nothing: not its file, nor a temp beside it,
/// whichever of its checks it's cancelled at, from its transforms' to the
/// writer's last; past those, it's written.
#[test]
fn a_cancelled_export_writes_nothing() {
    let dir = tempdir();
    let a = make_pdf(&dir, "a.pdf", "A", 3, &[]);
    let model = open(&a, &[], Box::new(FixedCount(10)));
    let out = dir.join("out.pdf");
    let tracker = in_progress::Tracker::new();
    tracker.cancel();
    let from = || model.export_from(Actor::User);
    let second = model.page(1).unwrap();
    for e in [
        Export::extract(from(), &[second], to(&out)),
        Export::flattened(from(), to(&out)),
        Export::whole(from(), to(&out)),
    ] {
        let err = run(e, &AskAnswers::default(), &tracker).unwrap_err();
        assert!(matches!(err, ExportError::Cancelled(_)), "{err}");
    }
    let names = || -> Vec<_> {
        (std::fs::read_dir(&dir).unwrap())
            .map(|e| e.unwrap().file_name())
            .collect()
    };
    assert_eq!(names(), ["a.pdf"]);
    for e in [
        Export::extract(from(), &[second], to(&out)),
        Export::flattened(from(), to(&out)),
    ] {
        let mut at = 1;
        let cancelled = loop {
            let checks = Arc::default();
            match run(e.clone(), &AskAnswers::default(), &CancelAt { checks, at }) {
                Ok(_) => break at - 1,
                Err(ExportError::Cancelled(_)) => assert_eq!(names(), ["a.pdf"], "at {at}"),
                Err(e) => panic!("{e}"),
            }
            at += 1;
        };
        assert!(cancelled > 3, "cancelled at {cancelled} checks");
        assert!(out.is_file());
        std::fs::remove_file(&out).unwrap();
    }
}

/// An export of a signed form asks before it breaks a signature, as a
/// Rewrite of it would; an extract that leaves out every signed field's
/// page has none to break, and doesn't. Print answers it itself.
#[test]
fn an_export_asks_only_for_the_signatures_it_exports() {
    let mut f = Form::new(2);
    let name = f.text("Name", 0, [72.0, 600.0, 272.0, 620.0]);
    f.fill(name, "Ada");
    let sig = f.signature("Signed", 1, [72.0, 100.0, 272.0, 140.0]);
    f.sign(sig);
    let model = crate::materialize::tests::open(&f.bytes(), None);
    let [first, second] = [0, 1].map(|i| model.page(i).unwrap());
    let from = || model.export_from(Actor::User);
    let err = run_it(Export::extract(from(), &[first, second], Out::Bytes)).unwrap_err();
    let ExportError::Ask(Ask::Signatures { impact, .. }) = err else {
        panic!("{err}");
    };
    assert_eq!(impact.len(), 1);
    assert_eq!(impact[0].signature, "Signed");
    let answers = AskAnswers {
        break_signatures: true,
        ..AskAnswers::default()
    };
    let both = Export::extract(from(), &[first, second], Out::Bytes);
    assert_eq!(run(both, &answers, &NoProgress).unwrap().pages, 2);
    let made = run_it(Export::extract(from(), &[first], Out::Bytes)).unwrap();
    assert_eq!(made.pages, 1);
    let print = printed(from(), &[first, second], true, &NoProgress).unwrap();
    assert_eq!(Pdf::load_mem(&print).unwrap().get_pages().len(), 2);
}

/// Each transform needs what an edit doing the same would: a plugin may
/// leave pages out, never take markup or sheaf's private data off, and an
/// OCR job may do neither. The gates say so before anything is lowered,
/// as do an op's own.
#[test]
fn a_transform_needs_its_capability() {
    let dir = tempdir();
    let a = make_pdf(&dir, "a.pdf", "A", 2, &[]);
    let model = open(&a, &[], Box::new(FixedCount(10)));
    let first = model.page(0).unwrap();
    let plugin = Actor::Plugin {
        id: "p".into(),
        version: "1.0".into(),
    };
    let gates = |actor: &Actor, e: fn(ExportFrom, Out) -> Export| {
        preflight(&e(model.export_from(actor.clone()), Out::Bytes))
    };
    let refused = |transform, cap| Gate::Refused { transform, cap };
    assert_eq!(
        gates(&plugin, Export::clean_copy),
        [
            refused("remove-markup", Cap::Approval),
            refused("remove-private-data", Cap::Approval)
        ]
    );
    assert_eq!(
        gates(&plugin, Export::flattened),
        [refused("flatten-annotations", Cap::Approval)]
    );
    let ocr = Actor::Job(crate::tx::JobKind::Ocr);
    let extract = Export::extract(model.export_from(ocr), &[first], Out::Bytes);
    assert_eq!(preflight(&extract), [refused("keep-pages", Cap::Plugin)]);
    let extract = Export::extract(model.export_from(plugin.clone()), &[first], Out::Bytes);
    assert_eq!(preflight(&extract), []);
    assert_eq!(run_it(extract).unwrap().pages, 1);
    let clean = Export::clean_copy(model.export_from(plugin), Out::Bytes);
    let err = run_it(clean).unwrap_err();
    assert!(
        matches!(err, ExportError::Refused(Gate::Refused { .. })),
        "{err}"
    );
}
