//! The Append ≡ Rewrite property (M10.5 §7.2, slice 10): after every save
//! in a random run of edits of every kind, batches, jobs, undo, redo and
//! jumps ([`Run`]), with saves as the plan decides, Save As, and Appends
//! forced as "Append anyway" forces them, the file written reopens as a
//! fresh Rewrite of the same state does. What's compared ([`Reopened`]):
//! the pages in order, each with its boxes, its content and resources by
//! value, and its annotations by name, with what they say and where links
//! and widgets' actions go, and where each annotation that goes somewhere
//! goes, the records' or not; the outline; the page labels; the field tree
//! and values; the form's own keys and whether it has an XFA part; the
//! header and footer; and where the open action goes.
//!
//! Over `Edit`s until slice 11 ports it to `Op`, with fixed cases beside it:
//! labels moved with their first page, links to pages a later Append
//! brings, a node split and then reordered, and the header and footer
//! over a reorder, which stands in for slice 17's live tokens. And a case
//! for each gap the property found: what a page shown as another page
//! changes (its links, bookmarks and number), fields parted from their
//! widgets or written over them, save policy's XFA drop applied and taken
//! back, a header's pages and the labels after a delete, a primary
//! object's place taken by another record of it, and a comment made
//! editable on a copy. And for the gaps slice 10's review found: a crop's
//! copy made the page's copy 0 by a move, and a copy's action, inline
//! link or screen annotation going to a page brought in or duplicated
//! after it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use in_progress::NoProgress;
use lopdf::{Dictionary, Document as Pdf, LazyPdf, Object, ObjectId, Stream, dictionary};
use proptest::prelude::*;

use super::append::Refused;
use super::plan::{Onto, Strategy, forced_append};
use super::removal::Mode;
use super::removal_tests::scratch;
use super::{
    AskAnswers, How, ObjKey, SaveReport, StrategyKind, Why, materialize, plan, removal, save,
    write_file,
};
use crate::annot::Annot;
use crate::file::ForeignAnnot;
use crate::forms::fixture::{Form, every_kind, every_kind_in, outline_tree};
use crate::forms::{FieldId, FieldKind, Value};
use crate::geom::Geom;
use crate::labels::{LabelRange, LabelSpec, Style};
use crate::model::history_tests::{self, Run, with_theirs};
use crate::model::{
    AnnotBody, DocState, Document, Edit, FieldProps, PageId, Rebase, SourceId, SourceInfo, Target,
    TextStyle, XfaState,
};
use crate::stamps::HeaderFooter;

/// Page `page`'s link of [`with_theirs`] goes to page `to`.
fn link_to(f: &mut Form, link: ObjectId, to: usize) {
    let to = f.pages[to];
    f.dict(link).set("Dest", vec![to.into(), "Fit".into()]);
}

/// [`every_kind`]'s push button, on the second page of `f`, its action a
/// GoTo of its own to page `to`.
fn button_to(f: &mut Form, to: usize) {
    let named = |d: &Dictionary| d.get(b"T").and_then(Object::as_str).ok() == Some(b"print[0]");
    let button = (f.annots(1).into_iter())
        .find(|&w| f.pdf.get_dictionary(w).is_ok_and(named))
        .expect("the push button");
    let to = f.pages[to];
    let action = f.pdf.add_object(dictionary! {
        "S" => "GoTo", "D" => vec![to.into(), "Fit".into()],
    });
    f.dict(button).set("A", action);
}

/// The files a run saves: the document's, six pages with [`every_kind`]'s
/// form on the first two, its push button going to the sixth page, with an
/// XFA part when `xfa` (which a change to the form drops, so a save
/// rewrites), other tools' annotations on each page ([`with_theirs`]),
/// each page's link going two pages on, and bookmarks to its first and
/// fourth pages; and the form whose pages come in, its two pages' links
/// to each other and its push button going to its first page. The fixed
/// cases' document, which has no open action ([`files_opening`]).
fn files(xfa: bool) -> (Vec<u8>, Vec<u8>) {
    files_opening(xfa, false)
}

/// [`files`], the document's open action going to its second page when
/// `open` (the property's: the fixed cases that append a page's delete,
/// replacement or rebase, which would leave the open action naming a page
/// outside the tree, are refused for it, slice 10's review).
fn files_opening(xfa: bool, open: bool) -> (Vec<u8>, Vec<u8>) {
    let mut f = every_kind_in(6);
    for page in 0..6 {
        let link = with_theirs(&mut f, page, ["theirs-1", "theirs-2"][page % 2]);
        link_to(&mut f, link, (page + 2) % 6);
    }
    button_to(&mut f, 5);
    if open {
        let (catalog, to) = (f.catalog, f.pages[1]);
        f.dict(catalog)
            .set("OpenAction", vec![to.into(), "Fit".into()]);
    }
    if xfa {
        f.xfa();
    }
    let item = |title: &str, page: ObjectId| {
        dictionary! {
            "Title" => Object::string_literal(title),
            "Dest" => vec![page.into(), "Fit".into()],
        }
    };
    let pages = f.pages.clone();
    outline_tree(
        &mut f,
        vec![(0, item("A", pages[0])), (0, item("B", pages[3]))],
    );
    let mut other = every_kind();
    for page in 0..2 {
        let link = with_theirs(&mut other, page, ["theirs-1", "theirs-2"][page]);
        link_to(&mut other, link, 1 - page);
    }
    button_to(&mut other, 0);
    (f.bytes(), other.bytes())
}

/// Where a link goes, as its file has it.
#[derive(Debug, PartialEq)]
enum Dest {
    /// A page of the file, by its place, and how it shows it.
    Page(usize, String),
    /// Anything else, as written.
    Other(String),
}

/// An annotation, as a file reopened holds it.
#[derive(Debug, PartialEq)]
enum AnnotSays {
    Mark(Box<Annot>),
    Foreign(Box<ForeignAnnot>, Option<Dest>),
    /// A widget: its field's full name, what it is beside it, and where
    /// its action goes.
    Widget(String, String, Option<Dest>),
}

/// A page, as a file reopened holds it.
#[derive(Debug, PartialEq)]
struct PageSays {
    geom: Geom,
    /// Its content streams, decoded.
    content: Vec<String>,
    /// Its resources, references followed.
    resources: String,
    annots: Vec<AnnotSays>,
    /// Where each entry of its `/Annots` that goes somewhere goes, by its
    /// subtype, in order: the records' and those the records don't hold
    /// (a screen's action), a dictionary written straight into the array
    /// among them.
    goes: Vec<(String, Dest)>,
}

/// A bookmark: its depth, title, the place of the page it goes to and
/// where on it (or "kept"), its color, style, and whether it's open.
type BookmarkSays = (
    u16,
    String,
    Option<String>,
    Option<[u8; 3]>,
    TextStyle,
    bool,
);

/// A field: its depth, its name, and what it holds when it's a terminal:
/// its kind, flags, value and default, what doesn't change when it's
/// filled, the places of the pages its widgets are on, and its widgets
/// elsewhere.
type FieldSays = (
    usize,
    String,
    Option<(
        FieldKind,
        u32,
        Option<Value>,
        Option<Value>,
        FieldProps,
        Vec<usize>,
        u16,
    )>,
);

/// What a file says, reopened, as the property compares it.
#[derive(Debug, PartialEq)]
struct Reopened {
    pages: Vec<PageSays>,
    outline: Vec<BookmarkSays>,
    labels: Option<Vec<LabelRange>>,
    fields: Vec<FieldSays>,
    /// The form's `/CO` by full name, `/DA`, `/Q`, `/NeedAppearances`,
    /// `/SigFlags`, and its XFA part.
    form: (Vec<String>, Option<Arc<str>>, u8, bool, i64, XfaState),
    /// The header and footer, and the places of the pages it's on.
    header: Option<(HeaderFooter, Option<Vec<usize>>)>,
    /// Where the catalog's `/OpenAction` goes, when it has one.
    open: Option<Dest>,
}

/// `o` of `pdf` written out by value, references followed `depth` deep,
/// a stream as its dictionary and its decoded bytes.
fn canon(pdf: &LazyPdf, o: &Object, depth: u8) -> String {
    match o {
        Object::Reference(id) if depth > 0 => match pdf.get(*id) {
            Ok(o) => canon(pdf, o, depth - 1),
            Err(_) => "null".into(),
        },
        Object::Dictionary(d) => dict(pdf, d, depth),
        Object::Array(items) => {
            let items: Vec<String> = items.iter().map(|o| canon(pdf, o, depth)).collect();
            format!("[{}]", items.join(" "))
        }
        Object::Stream(s) => format!("{} stream {}", dict(pdf, &s.dict, depth), decoded(s)),
        o => format!("{o:?}"),
    }
}

/// [`canon`] of a dictionary, its keys in order, but those that change
/// with how the stream it heads is written.
fn dict(pdf: &LazyPdf, d: &Dictionary, depth: u8) -> String {
    let mut entries: Vec<String> = (d.iter())
        .filter(|(k, _)| !matches!(k.as_slice(), b"Length" | b"Filter" | b"DecodeParms"))
        .map(|(k, v)| format!("/{} {}", String::from_utf8_lossy(k), canon(pdf, v, depth)))
        .collect();
    entries.sort();
    format!("<<{}>>", entries.join(" "))
}

/// A stream's bytes, decoded.
fn decoded(s: &Stream) -> String {
    let bytes = s
        .decompressed_content()
        .unwrap_or_else(|_| s.content.clone());
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Where the link `id` of `pdf`, among `pages`, goes.
fn dest(pdf: &LazyPdf, id: ObjectId, pages: &[ObjectId]) -> Option<Dest> {
    dest_of(pdf, pdf.get_dictionary(id).ok()?, pages)
}

/// Where `link`, an annotation of `pdf` (a link, or one with an action),
/// among `pages`, goes: its `/Dest`, or its action's `/D`.
fn dest_of(pdf: &LazyPdf, link: &Dictionary, pages: &[ObjectId]) -> Option<Dest> {
    let dest = match link.get(b"Dest") {
        Ok(dest) => dest,
        Err(_) => {
            let (_, action) = pdf.dereference(link.get(b"A").ok()?).ok()?;
            action.as_dict().ok()?.get(b"D").ok()?
        }
    };
    let (_, dest) = pdf.dereference(dest).ok()?;
    let page = (dest.as_array().ok())
        .and_then(|d| Some((d.first()?.as_reference().ok()?, &d[1..])))
        .and_then(|(page, view)| Some((pages.iter().position(|&p| p == page)?, view)));
    Some(match page {
        Some((at, view)) => Dest::Page(at, format!("{view:?}")),
        None => Dest::Other(canon(pdf, dest, 0)),
    })
}

/// The places of `pages` in `st`'s order.
fn places(st: &DocState, pages: &[PageId]) -> Vec<usize> {
    pages.iter().filter_map(|&p| st.pos_of(p)).collect()
}

/// What the file at `path` says, opened as the app opens it.
fn reopened(path: &Path) -> Reopened {
    let opened = crate::open::file(path, None).unwrap();
    assert!(opened.torn.is_none(), "{}", path.display());
    let st = opened.doc.state();
    let pages = (st.pages())
        .map(|(id, def, geom)| {
            let src = &def.base.src;
            let pdf = src.pdf();
            let facts = src.page(def.base.index).expect("a page of its file");
            let ids: Vec<ObjectId> = src.page_ids().collect();
            let content = (facts.contents.iter())
                .map(|&c| match pdf.get(c) {
                    Ok(Object::Stream(s)) => decoded(s),
                    o => format!("{o:?}"),
                })
                .collect();
            let resources =
                (facts.resources.as_deref()).map_or_else(String::new, |r| canon(pdf, r, 8));
            // The page's `/Annots`, as its file has it.
            let listed: &[Object] = (pdf.get_dictionary(facts.id).ok())
                .and_then(|d| d.get(b"Annots").ok())
                .and_then(|a| pdf.dereference(a).ok())
                .and_then(|(_, a)| a.as_array().ok())
                .map_or(&[], Vec::as_slice);
            let annots = (st.annots_on(id))
                .map(|(_, rec)| match &rec.body {
                    AnnotBody::Mark(m) => AnnotSays::Mark(Box::new(m.annot.clone())),
                    AnnotBody::Foreign(f) => {
                        let to = match &f.src {
                            Some(r) => dest(r.src.pdf(), r.id, &ids),
                            // Written straight into `/Annots`.
                            None => (listed.get(f.summary.slot))
                                .and_then(|e| e.as_dict().ok())
                                .and_then(|d| dest_of(pdf, d, &ids)),
                        };
                        AnnotSays::Foreign(Box::new((*f.summary).clone()), to)
                    }
                    AnnotBody::Widget(w) => {
                        let field = st.full_name(w.field).unwrap_or_default();
                        let says = (w.merged, w.rect, w.flags, &w.mk, &w.da, &w.state, &w.states);
                        let to = (w.src.as_ref()).and_then(|r| dest(r.src.pdf(), r.id, &ids));
                        AnnotSays::Widget(field, format!("{says:?} {:?}", w.props), to)
                    }
                })
                .collect();
            let goes = (listed.iter())
                .filter_map(|e| {
                    let d = pdf.dereference(e).ok()?.1.as_dict().ok()?;
                    let subtype = (d.get(b"Subtype").ok())
                        .and_then(|s| s.as_name().ok())
                        .map(|s| String::from_utf8_lossy(s).into_owned());
                    Some((subtype.unwrap_or_default(), dest_of(pdf, d, &ids)?))
                })
                .collect();
            PageSays {
                geom: **geom,
                content,
                resources,
                annots,
                goes,
            }
        })
        .collect();
    let outline = (st.bookmarks_preorder().into_iter())
        .map(|(depth, _, b)| {
            let target = b.target.as_ref().map(|t| match t {
                Target::Page { page, view } => format!("{:?} {view:?}", st.pos_of(*page)),
                Target::Kept => "kept".into(),
            });
            (depth, b.title.to_string(), target, b.color, b.style, b.open)
        })
        .collect();
    let mut fields = Vec::new();
    let mut stack: Vec<(usize, FieldId)> =
        (st.form().roots.iter().rev()).map(|&f| (0, f)).collect();
    while let Some((depth, f)) = stack.pop() {
        let Some(node) = st.field(f) else {
            continue;
        };
        let term = node.term.as_ref().map(|t| {
            let on: Vec<PageId> = (t.widgets.iter())
                .filter_map(|&w| Some(st.annot(w)?.page))
                .collect();
            let props = (*t.props).clone();
            let pages = places(st, &on);
            (
                t.kind,
                t.flags,
                t.value.clone(),
                t.default.clone(),
                props,
                pages,
                t.outside,
            )
        });
        fields.push((depth, node.partial.to_string(), term));
        stack.extend(node.kids.iter().rev().map(|&k| (depth + 1, k)));
    }
    let form = st.form();
    let calc = (form.calc_order.iter())
        .filter_map(|&f| st.full_name(f))
        .collect();
    let header = st.header_footer().map(|hf| {
        let on = hf.pages.as_deref().map(|pages| places(st, pages));
        let hf = HeaderFooter {
            pages: None,
            ..HeaderFooter::clone(hf)
        };
        (hf, on)
    });
    Reopened {
        pages,
        outline,
        labels: st.label_ranges(),
        fields,
        form: (
            calc,
            form.da.clone(),
            form.align,
            form.need_appearances,
            form.sig_flags,
            form.xfa,
        ),
        header,
        open: opened_at(&opened.doc),
    }
}

/// Where `doc`'s file's catalog's `/OpenAction`, an explicit destination
/// or an action, goes among its pages.
fn opened_at(doc: &Document) -> Option<Dest> {
    let src = &doc.source(doc.primary())?.src;
    let pdf = src.pdf();
    let open = pdf.catalog().ok()?.get(b"OpenAction").ok()?;
    let as_link = match pdf.dereference(open).ok()?.1 {
        Object::Array(_) => dictionary! { "Dest" => open.clone() },
        _ => dictionary! { "A" => open.clone() },
    };
    let pages: Vec<ObjectId> = src.page_ids().collect();
    dest_of(pdf, &as_link, &pages)
}

/// The file `doc` just saved reopens as a fresh Rewrite of its state does,
/// written beside it in `dir`, after `what`.
fn reopens_as_rewritten(doc: &Document, dir: &Path, what: &str) {
    assert!(!doc.is_dirty(), "{what}");
    let fresh = dir.join("fresh.pdf");
    std::fs::write(&fresh, How::Rewritten.write(doc)).unwrap();
    let saved = doc.file().expect("a saved file").0.clone();
    let differ = differences(&reopened(&saved), &reopened(&fresh));
    assert!(
        differ.is_empty(),
        "{what}, saved / fresh:\n{}",
        differ.join("\n")
    );
}

/// Each of `items`, written out.
fn list<T: std::fmt::Debug>(items: &[T]) -> Vec<String> {
    items.iter().map(|item| format!("{item:?}")).collect()
}

/// Where `saved` and `fresh` differ, part by part.
fn differences(saved: &Reopened, fresh: &Reopened) -> Vec<String> {
    let mut out = Vec::new();
    let mut part = |name: String, a: &dyn std::fmt::Debug, b: &dyn std::fmt::Debug| {
        let (a, b) = (format!("{a:?}"), format!("{b:?}"));
        if a != b {
            out.push(format!("{name}: {a}\n    / {b}"));
        }
    };
    if saved.pages.len() != fresh.pages.len() {
        part("pages".into(), &saved.pages.len(), &fresh.pages.len());
    }
    for (i, (a, b)) in saved.pages.iter().zip(&fresh.pages).enumerate() {
        part(format!("page {i}'s boxes"), &a.geom, &b.geom);
        part(format!("page {i}'s content"), &a.content, &b.content);
        part(format!("page {i}'s resources"), &a.resources, &b.resources);
        if a.annots.len() != b.annots.len() {
            let named = |annots: &[AnnotSays]| -> Vec<String> {
                (annots.iter())
                    .map(|a| match a {
                        AnnotSays::Mark(m) => format!("mark {}", m.name()),
                        AnnotSays::Foreign(f, to) => format!("{} {} {to:?}", f.subtype, f.name),
                        AnnotSays::Widget(field, ..) => format!("widget {field}"),
                    })
                    .collect()
            };
            part(
                format!("page {i}'s annotations"),
                &named(&a.annots),
                &named(&b.annots),
            );
            continue;
        }
        for (j, (x, y)) in a.annots.iter().zip(&b.annots).enumerate() {
            part(format!("page {i}'s annotation {j}"), x, y);
        }
        part(format!("where page {i}'s annotations go"), &a.goes, &b.goes);
    }
    for (name, a, b) in [
        ("bookmark", list(&saved.outline), list(&fresh.outline)),
        ("field", list(&saved.fields), list(&fresh.fields)),
    ] {
        match a.len() == b.len() {
            true => (a.iter().zip(&b).enumerate())
                .for_each(|(i, (x, y))| part(format!("{name} {i}"), x, y)),
            false => part(format!("{name}s"), &a, &b),
        }
    }
    part("labels".into(), &saved.labels, &fresh.labels);
    part("form".into(), &saved.form, &fresh.form);
    part("header".into(), &saved.header, &fresh.header);
    part("the open action".into(), &saved.open, &fresh.open);
    out
}

/// Answers to whatever a save of a run asks: nothing it saves is signed,
/// and the XFA part's values are its fields'.
fn answers() -> AskAnswers {
    AskAnswers {
        break_signatures: true,
        drop_xfa_values: true,
        ..Default::default()
    }
}

/// `doc` saved to `path` as its plan says. A Rewrite is first run through
/// the removal check as a test oracle (§11.9), against the file saved: it
/// finds no derived object's text gone (a mark's, a block's) unless the
/// plan says something was removed or replaced. (What it finds of the
/// files' own objects isn't judged here: their words are on other pages
/// too, and a Rewrite drops what nothing reaches.)
fn planned(doc: &mut Document, path: &Path) -> SaveReport {
    let v = doc.current_version();
    let input = doc.save_input(v).unwrap();
    let target = doc.save_target(path);
    let plan = plan(&input, &target);
    let saved = (input.saved.as_ref()).filter(|s| s.record.knows_file());
    if let (Strategy::Rewrite { why }, Some(saved)) = (&plan.strategy, saved) {
        let file = std::fs::read(&saved.record.path).unwrap().into();
        let out = materialize(&input, &plan, target, &NoProgress).unwrap();
        let found = removal::of_rewrite(&input, &out, file).unwrap();
        let removes = (why.iter()).any(|w| matches!(w, Why::Removes(_) | Why::ReplacesText(_)));
        let derived = found.is_some_and(|f| f.derived);
        assert!(removes || !derived, "{why:?}: {found:?}");
    }
    doc.abandon_save(v);
    let report = save(doc, path, &answers(), &NoProgress);
    report.unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `doc` saved to `path` as its plan says, which appends.
fn appends(doc: &mut Document, path: &Path) {
    let report = planned(doc, path);
    assert_eq!(
        report.strategy,
        Some(StrategyKind::Append),
        "{:?}",
        report.why
    );
}

/// `doc` appended to the file it was saved as, or onto a copy of it at
/// `path`, as "Append anyway" appends (save policy's transforms in the
/// section, the removal check reporting): a Rewrite instead when the
/// section can't follow the file. How it wrote.
fn forced(doc: &mut Document, path: &Path) -> Strategy {
    let v = doc.current_version();
    let input = doc.save_input(v).unwrap();
    let target = doc.save_target(path);
    let onto = match target.same_file {
        true => Onto::Saved,
        false => Onto::CopyOf,
    };
    let plan = forced_append(&input, onto);
    let answers = AskAnswers {
        append_anyway: true,
        stays_readable: true,
        found: true,
        ..answers()
    };
    match write_file(&input, &plan, target, &answers, &NoProgress) {
        Ok(written) => {
            let strategy = written.strategy.clone();
            written.record_in(doc).unwrap();
            strategy
        }
        Err(e) => {
            doc.abandon_save(v);
            panic!("{}: {e}", path.display())
        }
    }
}

/// What a property run does beside [`Run::act`]'s actions, up to
/// [`history_tests::TRIM`]: a save as planned to the document's file, Save
/// As to the other, and an Append forced onto either, each three times as
/// likely as one kind of edit.
const SAVES: u8 = history_tests::TRIM + 1;
const ACTIONS: u8 = SAVES + 9;

/// A run of `actions` over [`files`], with an XFA part when `xfa`: the
/// saves compared as they're made.
fn run(xfa: bool, actions: &[(u8, u64)]) {
    // Mark ids come from the thread's generator: the same ones each run.
    fastrand::seed(1);
    let dir = scratch();
    let (primary, other) = files_opening(xfa, true);
    let a = dir.join("a.pdf");
    std::fs::write(&a, primary).unwrap();
    let b = dir.join("b.pdf");
    let mut r = Run::over(crate::open::file(&a, None).unwrap().doc, other);
    for (i, &(action, seed)) in actions.iter().enumerate() {
        if action < SAVES {
            r.act((action, seed));
            continue;
        }
        let rng = &mut fastrand::Rng::with_seed(seed);
        let file = r.d.file().expect("a file").0.to_path_buf();
        let elsewhere = if file == a { &b } else { &a };
        let how = match (action - SAVES) % 3 {
            0 => {
                let report = planned(&mut r.d, &file);
                format!("{:?} {:?} in place", report.strategy, report.why)
            }
            1 => {
                let report = planned(&mut r.d, elsewhere);
                format!("{:?} {:?} as another", report.strategy, report.why)
            }
            _ => {
                let to = if rng.bool() { &file } else { elsewhere };
                format!("{:?} forced to {}", forced(&mut r.d, to), to.display())
            }
        };
        reopens_as_rewritten(&r.d, &dir, &format!("action {i}, a save: {how}"));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn runs() -> impl proptest::strategy::Strategy<Value = Vec<(u8, u64)>> {
    prop::collection::vec((0..ACTIONS, any::<u64>()), 1..40)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn append_rewrite_reopen_alike(xfa in any::<bool>(), actions in runs()) {
        run(xfa, &actions);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    #[test]
    #[ignore = "1,024 cases: cargo test -p sheaf-doc --release -- --ignored append_rewrite"]
    fn append_rewrite_reopen_alike_1024(xfa in any::<bool>(), actions in runs()) {
        run(xfa, &actions);
    }
}

/// [`files`]' document, its form with an XFA part when `xfa`, opened from
/// `dir`, and its file's path.
fn opened(dir: &Path, xfa: bool) -> (PathBuf, Document) {
    let path = dir.join("a.pdf");
    std::fs::write(&path, files(xfa).0).unwrap();
    (path.clone(), crate::open::file(&path, None).unwrap().doc)
}

/// Pages i–iii labelled in roman, the rest from 1; then the fourth page
/// moved before the first, its range starting with it and the roman one
/// after it: each saved by an Append, which writes the labels again.
#[test]
fn append_rewrite_labels_follow_a_page_moved_first() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, true);
    let [first, fourth] = [0, 3].map(|i| doc.page(i).unwrap());
    let range = |start, style| LabelSpec {
        start,
        style,
        prefix: String::new(),
        first: 1,
    };
    let labels = Edit::SetPageLabels {
        ranges: vec![
            range(first, Style::LowerRoman),
            range(fourth, Style::Decimal),
        ],
    };
    doc.apply(labels, "labels").unwrap();
    appends(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "labelled");
    let labels = doc.state().page_labels().unwrap();
    assert_eq!(labels, ["i", "ii", "iii", "1", "2", "3"]);
    let first = Edit::MovePages {
        ids: vec![fourth],
        before: Some(first),
    };
    doc.apply(first, "move").unwrap();
    appends(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "moved");
    let labels = doc.state().page_labels().unwrap();
    assert_eq!(labels, ["1", "i", "ii", "iii", "iv", "v"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The other file's first page, whose link goes to its second, appended
/// alone: the link goes nowhere written, so it's left out. Its second
/// page appended later: both links are written, each to the other's page.
#[test]
fn append_rewrite_links_to_pages_a_later_append_brings() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, true);
    // A file with no form, which an insert into the hybrid form would drop
    // the XFA part for, a Rewrite.
    let mut f = Form::new(2);
    for page in 0..2 {
        let link = with_theirs(&mut f, page, ["theirs-1", "theirs-2"][page]);
        link_to(&mut f, link, 1 - page);
    }
    let b = dir.join("b.pdf");
    std::fs::write(&b, f.bytes()).unwrap();
    let other = SourceInfo::read(doc.store(), &b, None).unwrap().0;
    let other = doc.add_source(other);
    let links = |doc: &Document| -> usize {
        let st = doc.state();
        (st.annots())
            .filter(|(_, rec)| rec.foreign().is_some_and(|f| f.summary.is_link()))
            .count()
    };
    let before = links(&doc);
    for range in [0..1, 1..2] {
        let insert = Edit::InsertPages {
            source: other,
            range,
            before: None,
            outline: None,
            annots: Vec::new(),
        };
        doc.apply(insert, "insert").unwrap();
        appends(&mut doc, &path);
        reopens_as_rewritten(&doc, &dir, "inserted");
    }
    let back = crate::open::file(&path, None).unwrap().doc;
    assert_eq!(links(&back), before + 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file of `n` pages, one node holding them, each saying its number.
fn pages_file(n: usize) -> Vec<u8> {
    let mut f = Form::new(n);
    f.bytes()
}

/// A node grown past its size at open splits as an Append writes it; a
/// reorder across the split, appended next, reopens as a Rewrite does.
#[test]
fn append_rewrite_a_split_then_a_reorder() {
    let dir = scratch();
    let path = dir.join("a.pdf");
    std::fs::write(&path, pages_file(130)).unwrap();
    let mut doc = crate::open::file(&path, None).unwrap().doc;
    let fifth = doc.page(4).unwrap();
    doc.apply(Edit::DuplicatePages { ids: vec![fifth] }, "duplicate")
        .unwrap();
    appends(&mut doc, &path);
    assert!(doc.saved_record().unwrap().tree.nodes.len() > 1, "split");
    reopens_as_rewritten(&doc, &dir, "split");
    let [first, last] = [0, 130].map(|i| doc.page(i).unwrap());
    let reorder = Edit::MovePages {
        ids: vec![last, fifth],
        before: Some(first),
    };
    doc.apply(reorder, "reorder").unwrap();
    appends(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "reordered");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The header and footer, counting pages, over a reorder: a Rewrite for
/// the legacy header and footer, which draws every page's numbers again.
/// It stands in for slice 17's live tokens, whose reorder appends what
/// their layout changed.
#[test]
fn append_rewrite_a_header_over_a_reorder() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, true);
    let hf = HeaderFooter {
        texts: ["Page <<1>> of <<N>>", "", "", "", "", ""].map(String::from),
        ..Default::default()
    };
    let set = Edit::SetHeaderFooter {
        header_footer: Some(hf),
    };
    doc.apply(set, "header").unwrap();
    let report = save(&mut doc, &path, &answers(), &NoProgress).unwrap();
    assert_eq!(report.why, [Why::LegacyHeaderFooter]);
    reopens_as_rewritten(&doc, &dir, "header");
    let [first, last] = [0, 5].map(|i| doc.page(i).unwrap());
    let reorder = Edit::MovePages {
        ids: vec![last],
        before: Some(first),
    };
    doc.apply(reorder, "reorder").unwrap();
    let report = save(&mut doc, &path, &answers(), &NoProgress).unwrap();
    assert_eq!(report.why, [Why::LegacyHeaderFooter]);
    reopens_as_rewritten(&doc, &dir, "reordered");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same edits each way give the same reopen facts, and the facts tell
/// a page's boxes, content and links apart: the comparison has teeth.
#[test]
fn append_rewrite_facts_tell_files_apart() {
    let dir = scratch();
    let (path, _) = opened(&dir, true);
    let base = reopened(&path);
    let turned = dir.join("turned.pdf");
    let mut f = Pdf::load(&path).unwrap();
    let first = f.get_pages()[&1];
    f.get_dictionary_mut(first).unwrap().set("Rotate", 90);
    f.save(&turned).unwrap();
    assert_ne!(reopened(&turned).pages[0].geom, base.pages[0].geom);
    let _ = std::fs::remove_dir_all(&dir);
}

/// An Append onto the document's file.
const APPENDED: Strategy = Strategy::Append { onto: Onto::Saved };

/// [`files`]' other file, a form, beside the document in `dir`,
/// registered in `doc`.
fn other(doc: &mut Document, dir: &Path) -> SourceId {
    let path = dir.join("form.pdf");
    std::fs::write(&path, files(false).1).unwrap();
    let info = SourceInfo::read(doc.store(), &path, None).unwrap().0;
    doc.add_source(info)
}

/// Page `i` of `doc` shown as page `index` of registration `source`.
fn rebase(doc: &mut Document, i: usize, source: SourceId, index: u32) {
    let id = doc.page(i).unwrap();
    let pages = vec![Rebase { id, source, index }];
    doc.apply(Edit::RebasePages { pages }, "rebase").unwrap();
}

/// The fourth page, which bookmark B and the second page's link go to,
/// made to show the sixth page again: no page is the fourth page's object
/// now, so the link to it goes, as a Rewrite leaves it out; and the page
/// is written at a number of its own, so the bookmark is written again,
/// going there.
#[test]
fn append_rewrite_a_rebased_page_s_links_go_and_bookmarks_follow() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, false);
    let primary = doc.primary();
    rebase(&mut doc, 3, primary, 5);
    assert_eq!(forced(&mut doc, &path), APPENDED);
    reopens_as_rewritten(&doc, &dir, "rebased");
    let back = crate::open::file(&path, None).unwrap().doc;
    let st = back.state();
    let b = (st.bookmarks_preorder().into_iter()).find(|(_, _, b)| &*b.title == "B");
    assert_eq!(b.and_then(|(_, _, b)| b.page()), back.page(3));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A page a Rewrite wrote at the primary object it was, made to show
/// another page: the next Rewrite writes it at a number of its own, the
/// old one being an object it no longer is.
#[test]
fn append_rewrite_a_rebased_page_leaves_its_number_to_a_rewrite() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, false);
    let copy = dir.join("b.pdf");
    assert_eq!(planned(&mut doc, &copy).why, [Why::OtherTarget]);
    let primary = doc.primary();
    rebase(&mut doc, 3, primary, 5);
    let report = planned(&mut doc, &path);
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    reopens_as_rewritten(&doc, &dir, "rewritten");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The other file's second page brought in and saved by a Rewrite, which
/// writes each of its fields with its one widget as one object; brought
/// in again, each field shows two widgets, so its object is no longer the
/// first widget's: the Append writes that widget anew, and its page,
/// which lists it.
#[test]
fn append_rewrite_a_page_brought_in_again_parts_fields_from_widgets() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, true);
    let other = other(&mut doc, &dir);
    let insert = || Edit::InsertPages {
        source: other,
        range: 1..2,
        before: None,
        outline: None,
        annots: Vec::new(),
    };
    doc.apply(insert(), "insert").unwrap();
    let copy = dir.join("b.pdf");
    assert_eq!(
        planned(&mut doc, &copy).strategy,
        Some(StrategyKind::Rewrite)
    );
    doc.apply(insert(), "insert").unwrap();
    appends(&mut doc, &copy);
    reopens_as_rewritten(&doc, &dir, "inserted again");
    let _ = std::fs::remove_dir_all(&dir);
}

/// After a Rewrite, the first page's fields shown twice more: the Append
/// writes each field read merged with its one widget over its own object,
/// which the Rewrite's map lists by the widget.
#[test]
fn append_rewrite_fields_after_a_rewrite_are_written_over_themselves() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, true);
    let other = other(&mut doc, &dir);
    rebase(&mut doc, 1, other, 1);
    let copy = dir.join("b.pdf");
    planned(&mut doc, &copy);
    let first = doc.page(0).unwrap();
    doc.apply(
        Edit::DuplicatePages {
            ids: vec![first, first],
        },
        "duplicate",
    )
    .unwrap();
    assert_eq!(forced(&mut doc, &copy), APPENDED);
    reopens_as_rewritten(&doc, &dir, "duplicated");
    let _ = std::fs::remove_dir_all(&dir);
}

/// After a Rewrite dropped the XFA part, which put its tooltips in the
/// fields' `/TU`, a page with fields duplicated: the copies the Append
/// writes have the tooltips too, as a Rewrite's would.
#[test]
fn append_rewrite_fields_written_after_an_xfa_drop_have_its_tooltips() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, true);
    let last = doc.page(5).unwrap();
    doc.apply(Edit::DeletePages { ids: vec![last] }, "delete")
        .unwrap();
    let other = other(&mut doc, &dir);
    let insert = Edit::InsertPages {
        source: other,
        range: 0..1,
        before: None,
        outline: None,
        annots: Vec::new(),
    };
    doc.apply(insert, "insert").unwrap();
    assert_eq!(
        planned(&mut doc, &path).strategy,
        Some(StrategyKind::Rewrite)
    );
    let first = doc.page(0).unwrap();
    doc.apply(Edit::DuplicatePages { ids: vec![first] }, "duplicate")
        .unwrap();
    appends(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "duplicated");
    let back = crate::open::file(&path, None).unwrap().doc;
    let st = back.state();
    let copied: Vec<Option<String>> = (st.terminals())
        .filter(|&(f, _, _)| st.full_name(f).is_some_and(|n| n.starts_with("a.")))
        .map(|(_, _, t)| t.props.tooltip.clone())
        .collect();
    assert_eq!(copied.len(), 5);
    assert!(copied.iter().all(Option::is_some), "{copied:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A form page deleted and appended anyway, which drops the XFA part; the
/// delete undone and appended anyway again: the fields are as the file had
/// them, so save policy keeps the XFA part, and the Append brings it back,
/// with the fields' own tooltips, as a Rewrite would write them.
#[test]
fn append_rewrite_an_xfa_part_policy_keeps_again_comes_back() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, true);
    let second = doc.page(1).unwrap();
    doc.apply(Edit::DeletePages { ids: vec![second] }, "delete")
        .unwrap();
    assert_eq!(forced(&mut doc, &path), APPENDED);
    reopens_as_rewritten(&doc, &dir, "deleted");
    let xfa = |path: &Path| {
        crate::open::file(path, None)
            .unwrap()
            .doc
            .state()
            .form()
            .xfa
    };
    assert!(!xfa(&path).present);
    doc.undo().unwrap();
    assert_eq!(forced(&mut doc, &path), APPENDED);
    reopens_as_rewritten(&doc, &dir, "undone");
    assert!(xfa(&path).present);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A header and footer on one page, saved by a Rewrite; a page before it
/// deleted: the catalog, which says its pages by their places, is written
/// again.
#[test]
fn append_rewrite_a_header_s_pages_follow_a_delete() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, false);
    let fifth = doc.page(4).unwrap();
    let hf = HeaderFooter {
        texts: ["On one page", "", "", "", "", ""].map(String::from),
        pages: Some(vec![fifth]),
        ..Default::default()
    };
    let set = Edit::SetHeaderFooter {
        header_footer: Some(hf),
    };
    doc.apply(set, "header").unwrap();
    assert_eq!(planned(&mut doc, &path).why, [Why::LegacyHeaderFooter]);
    let first = doc.page(0).unwrap();
    doc.apply(Edit::DeletePages { ids: vec![first] }, "delete")
        .unwrap();
    assert_eq!(forced(&mut doc, &path), APPENDED);
    reopens_as_rewritten(&doc, &dir, "deleted");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The page labels' one range starting at the third page, appended; that
/// page deleted: no page starts a range now, and the catalog, written
/// again, names no labels.
#[test]
fn append_rewrite_labels_go_with_the_page_they_start_at() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, false);
    let third = doc.page(2).unwrap();
    let labels = Edit::SetPageLabels {
        ranges: vec![LabelSpec {
            start: third,
            style: Style::Decimal,
            prefix: String::new(),
            first: 1,
        }],
    };
    doc.apply(labels, "labels").unwrap();
    appends(&mut doc, &path);
    doc.apply(Edit::DeletePages { ids: vec![third] }, "delete")
        .unwrap();
    assert_eq!(forced(&mut doc, &path), APPENDED);
    reopens_as_rewritten(&doc, &dir, "deleted");
    let back = crate::open::file(&path, None).unwrap().doc;
    assert_eq!(back.state().label_ranges(), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// After a Rewrite, the fifth page made to show the other file's first,
/// and the third replaced by the primary's fourth and fifth pages, the
/// fifth shown by no page: the new page is that page's object, and the
/// fifth page, whose number in the file that was, is written at a number
/// of its own.
#[test]
fn append_rewrite_a_primary_page_s_number_goes_to_the_page_it_is() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, false);
    let other = other(&mut doc, &dir);
    let copy = dir.join("b.pdf");
    planned(&mut doc, &copy);
    rebase(&mut doc, 4, other, 0);
    let third = doc.page(2).unwrap();
    let primary = doc.primary();
    let replace = Edit::ReplacePages {
        ids: vec![third],
        source: primary,
        range: 3..5,
        annots: Vec::new(),
    };
    doc.apply(replace, "replace").unwrap();
    let fifth = doc.page(5).unwrap();
    let was = doc.saved_record().unwrap().map.keys[&ObjKey::Page(fifth)];
    assert_eq!(forced(&mut doc, &copy), APPENDED);
    let map = &doc.saved_record().unwrap().map;
    assert_ne!(map.keys[&ObjKey::Page(fifth)], was);
    reopens_as_rewritten(&doc, &dir, "replaced");
    let _ = std::fs::remove_dir_all(&dir);
}

/// After a Rewrite, the first page's form shown by the fifth page too,
/// then the first replaced by its own page again: the replacement's
/// widgets, first in order, are the primary's objects the fifth page's
/// were in the file, which the Append doesn't write: a Rewrite, for the
/// objects the file has elsewhere.
#[test]
fn append_rewrite_widgets_whose_objects_moved_rewrite() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, false);
    let other = other(&mut doc, &dir);
    let [first, fifth] = [0, 4].map(|i| doc.page(i).unwrap());
    let primary = doc.primary();
    let pages = vec![
        Rebase {
            id: fifth,
            source: primary,
            index: 0,
        },
        Rebase {
            id: first,
            source: other,
            index: 0,
        },
    ];
    doc.apply(Edit::RebasePages { pages }, "rebase").unwrap();
    let copy = dir.join("b.pdf");
    planned(&mut doc, &copy);
    let replace = Edit::ReplacePages {
        ids: vec![first],
        source: primary,
        range: 0..1,
        annots: Vec::new(),
    };
    doc.apply(replace, "replace").unwrap();
    let missing = Strategy::Rewrite {
        why: vec![Why::BaseObjectsMissing],
    };
    assert_eq!(forced(&mut doc, &copy), missing);
    reopens_as_rewritten(&doc, &dir, "replaced");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The second page brought in again, and the copy's reply of another tool
/// made editable: the copy's object is the original's, so the mark is one
/// of its own, and the original's reply stays another tool's.
#[test]
fn append_rewrite_a_copied_comment_made_editable_is_its_own() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, false);
    let primary = doc.primary();
    let insert = Edit::InsertPages {
        source: primary,
        range: 1..2,
        before: None,
        outline: None,
        annots: Vec::new(),
    };
    doc.apply(insert, "insert").unwrap();
    appends(&mut doc, &path);
    let copy = doc.page(6).unwrap();
    let (id, mark) = (doc.state().foreign_on(copy))
        .find(|(_, f)| f.summary.name == "theirs-2-reply")
        .map(|(id, f)| (id, f.summary.mark.clone().unwrap()))
        .unwrap();
    assert!(doc.state().annot(id).unwrap().foreign().unwrap().copy > 0);
    let annot = Annot { id, ..*mark };
    doc.apply(Edit::AnnotAdd { page: copy, annot }, "edit")
        .unwrap();
    let made = doc.state().marks_on(copy).next().unwrap().id;
    assert!(
        doc.state()
            .annot(made)
            .unwrap()
            .mark_record()
            .unwrap()
            .src
            .is_none()
    );
    assert_eq!(forced(&mut doc, &path), APPENDED);
    reopens_as_rewritten(&doc, &dir, "made editable");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The first two pages made to trade the primary's page objects, the order
/// as it was: the first shows the second page (by way of the third), and
/// the second the first. Each is written at the other's number, so the
/// saved node lists them the other way round: the Append splices it.
#[test]
fn append_rewrite_pages_that_trade_their_objects_keep_their_order() {
    let dir = scratch();
    let path = dir.join("a.pdf");
    std::fs::write(&path, pages_file(3)).unwrap();
    let mut doc = crate::open::file(&path, None).unwrap().doc;
    let primary = doc.primary();
    rebase(&mut doc, 0, primary, 2);
    rebase(&mut doc, 1, primary, 0);
    rebase(&mut doc, 0, primary, 1);
    appends(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "traded");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Another file's two pages brought in, the first's link going to the
/// second by an action of its own; then its first page again, whose link
/// goes to the second page's only copy; then its second page again: the
/// copy's link goes to that copy now, so the Append writes its action
/// again, as a Rewrite names it.
#[test]
fn append_rewrite_a_copy_s_link_action_follows_its_page() {
    let dir = scratch();
    let path = dir.join("a.pdf");
    std::fs::write(&path, pages_file(1)).unwrap();
    let mut doc = crate::open::file(&path, None).unwrap().doc;
    let mut f = Form::new(2);
    let link = with_theirs(&mut f, 0, "theirs");
    let to = f.pages[1];
    let action = f.pdf.add_object(dictionary! {
        "S" => "GoTo", "D" => vec![to.into(), "Fit".into()],
    });
    f.dict(link).set("A", action);
    let b = dir.join("b.pdf");
    std::fs::write(&b, f.bytes()).unwrap();
    let other = SourceInfo::read(doc.store(), &b, None).unwrap().0;
    let other = doc.add_source(other);
    for range in [0..2, 0..1, 1..2] {
        let insert = Edit::InsertPages {
            source: other,
            range,
            before: None,
            outline: None,
            annots: Vec::new(),
        };
        doc.apply(insert, "insert").unwrap();
        appends(&mut doc, &path);
        reopens_as_rewritten(&doc, &dir, "inserted");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file of two pages, the first named by what `holder` puts in it,
/// given the file and its first page's object. The first page duplicated,
/// and both cropped by one crop, whose two pages are both made from the
/// first page: the first in order is its copy 0, the page its object is,
/// and the duplicate copy 1. Saved; then the duplicate moved first, which
/// makes it copy 0 and the first copy 1, though neither is written (one
/// node holds both). The file's number for the first page's object still
/// holds the first, where what names it by number goes, while a Rewrite
/// writes the duplicate there: the Append is refused (finding 6 of slice
/// 10's review), and the save is a Rewrite.
fn a_crop_s_copy_made_copy_0_by_a_move(
    holder: impl FnOnce(&mut Form, ObjectId),
    want: impl FnOnce(&Document, PageId) -> Refused,
) {
    use crate::store::{BlobKind, MemoryKind, Origin};
    let dir = scratch();
    let path = dir.join("a.pdf");
    let mut f = Form::new(2);
    let to = f.pages[0];
    holder(&mut f, to);
    std::fs::write(&path, f.bytes()).unwrap();
    let mut doc = crate::open::file(&path, None).unwrap().doc;
    let first = doc.page(0).unwrap();
    doc.apply(Edit::DuplicatePages { ids: vec![first] }, "duplicate")
        .unwrap();
    let copy = doc.page(1).unwrap();
    assert_eq!(doc.state().def(copy).unwrap().copy, 1);
    let primary = doc.primary();
    let info = SourceInfo::clone(doc.source(primary).unwrap());
    let crop = [0.0, 0.0, 400.0, 500.0];
    let bytes = crate::save::cropped_copy(&info, &[(0, crop), (0, crop)]).unwrap();
    let kind = BlobKind::Pdf {
        password: None,
        origin: Origin::Memory(MemoryKind::Derived),
    };
    let cropped = SourceInfo {
        src: doc.store().open(Arc::from(bytes), kind).unwrap(),
        name: "a.pdf".into(),
        derived: Some(crate::Derived {
            from: primary,
            pages: vec![0, 0],
        }),
    };
    let source = doc.add_source(cropped);
    let pages = vec![
        Rebase {
            id: first,
            source,
            index: 0,
        },
        Rebase {
            id: copy,
            source,
            index: 1,
        },
    ];
    doc.apply(Edit::RebasePages { pages }, "crop").unwrap();
    planned(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "cropped");
    let moved = Edit::MovePages {
        ids: vec![copy],
        before: Some(first),
    };
    doc.apply(moved, "move").unwrap();
    // `Refused::why` is `BaseObjectsMissing` for several refusals: which
    // one stops it.
    let want = want(&doc, copy);
    assert_eq!(refusal(&doc), Some(want));
    let report = planned(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "moved");
    assert_eq!(
        (report.strategy, report.why),
        (Some(StrategyKind::Rewrite), vec![Why::BaseObjectsMissing])
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Why an Append of `doc`'s state onto the file it was saved as, the
/// removal check vetoing, is refused ([`Refused`]): None when it isn't.
fn refusal(doc: &Document) -> Option<Refused> {
    let input = doc.recovery_input();
    let path = doc.file().expect("a file").0.to_path_buf();
    let plan = forced_append(&input, Onto::Saved);
    let base: Arc<[u8]> = std::fs::read(&path).unwrap().into();
    let out = super::append::append(&input, &plan, Arc::new(base), Mode::Veto, &NoProgress);
    let e = out.err()?;
    let refused = e.chain().find_map(|c| c.downcast_ref::<Refused>()).cloned();
    Some(refused.unwrap_or_else(|| panic!("not refused: {e:#}")))
}

/// The second page's link going to the first, which the Append writes
/// again (the first page's copies came and went), its destination the
/// first page's object. It's the primary's copy 0, whose references are
/// numbers, so `Closure::stale` refuses it before `held` would: the
/// open action's case below is `held`'s.
#[test]
fn append_rewrite_a_crop_s_copy_made_copy_0_by_a_move_rewrites() {
    a_crop_s_copy_made_copy_0_by_a_move(
        |f, to| {
            let link = f.pdf.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Link",
                "Rect" => vec![100.into(), 10.into(), 120.into(), 30.into()],
                "Dest" => vec![to.into(), "Fit".into()],
            });
            f.set_annots(1, &[link]);
        },
        |doc, _| {
            // The link's page, third now.
            let st = doc.state();
            let (link, _) = st.foreign_on(doc.page(2).unwrap()).next().unwrap();
            Refused::Stale(ObjKey::Annot(link))
        },
    );
}

/// The catalog's `/OpenAction` going to the first page, which the
/// catalog's keys sheaf doesn't own keep as the file has them: `held`
/// refuses it, the duplicate's page not at the first page's object.
#[test]
fn append_rewrite_an_open_action_to_a_crop_s_copy_made_copy_0_rewrites() {
    a_crop_s_copy_made_copy_0_by_a_move(
        |f, to| {
            let catalog = f.catalog;
            f.dict(catalog)
                .set("OpenAction", vec![to.into(), "Fit".into()]);
        },
        |_, copy| Refused::Held(ObjKey::Page(copy)),
    );
}

/// [`append_rewrite_a_copy_s_link_action_follows_its_page`] with what
/// `holder` puts on the other file's first page in place of the link,
/// given the file and a GoTo action to its second page. Its two pages
/// brought in, then its first page again, whose copy of the action goes
/// to the second page's only copy, saved so; then its second page again.
/// When the copy's action is read as the page's copy, it goes to that
/// copy now, which the Append wouldn't write again (finding 7 of slice
/// 10's review): `refused`, and the save is a Rewrite. Else it's an
/// Append.
fn a_copy_s_action_follows_its_page(holder: impl FnOnce(&mut Form, ObjectId), refused: bool) {
    let dir = scratch();
    let path = dir.join("a.pdf");
    std::fs::write(&path, pages_file(1)).unwrap();
    let mut doc = crate::open::file(&path, None).unwrap().doc;
    let mut f = Form::new(2);
    let to = f.pages[1];
    let action = f.pdf.add_object(dictionary! {
        "S" => "GoTo", "D" => vec![to.into(), "Fit".into()],
    });
    holder(&mut f, action);
    let b = dir.join("b.pdf");
    std::fs::write(&b, f.bytes()).unwrap();
    let other = SourceInfo::read(doc.store(), &b, None).unwrap().0;
    let other = doc.add_source(other);
    for (i, range) in [0..2, 0..1, 1..2].into_iter().enumerate() {
        let insert = Edit::InsertPages {
            source: other,
            range,
            before: None,
            outline: None,
            annots: Vec::new(),
        };
        doc.apply(insert, "insert").unwrap();
        let report = planned(&mut doc, &path);
        reopens_as_rewritten(&doc, &dir, &format!("insert {i}"));
        if i == 2 {
            let want = match refused {
                true => (Some(StrategyKind::Rewrite), vec![Why::BaseObjectsMissing]),
                false => (Some(StrategyKind::Append), vec![]),
            };
            assert_eq!((report.strategy, report.why), want);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A push button whose action goes to the other page: the pages brought in
/// again join the form's instance the first brought, so the copy's button
/// is read as that instance's, copy 0, and its action goes to the second
/// page's copy 0 still, which both write: an Append.
#[test]
fn append_rewrite_a_copy_s_button_of_the_first_instance_appends() {
    a_copy_s_action_follows_its_page(
        |f, action| {
            let button = f.push_button("go", 0, crate::forms::fixture::row(0), "Go");
            f.dict(button).set("A", action);
        },
        false,
    );
}

/// A link written straight into the page's `/Annots`, its action going to
/// the other page.
#[test]
fn append_rewrite_a_copy_s_inline_link_follows_its_page() {
    a_copy_s_action_follows_its_page(
        |f, action| {
            let link = dictionary! {
                "Type" => "Annot", "Subtype" => "Link",
                "Rect" => vec![100.into(), 10.into(), 120.into(), 30.into()],
                "A" => action,
            };
            let page = f.pages[0];
            f.dict(page).set("Annots", vec![Object::Dictionary(link)]);
        },
        true,
    );
}

/// A screen annotation, which the records don't hold, its action going to
/// the other page.
#[test]
fn append_rewrite_a_copy_s_screen_action_follows_its_page() {
    a_copy_s_action_follows_its_page(
        |f, action| {
            let screen = f.pdf.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Screen",
                "Rect" => vec![100.into(), 10.into(), 120.into(), 30.into()],
                "A" => action,
            });
            f.set_annots(0, &[screen]);
        },
        true,
    );
}

/// A push button on the first page whose action goes to the second. The
/// first page duplicated: the copy's button is a new instance of the
/// form's, copy 1, whose action goes to the second page's only copy, and
/// that's saved. The second page duplicated: the copy's action goes to
/// that duplicate now, as a Rewrite names it, which the Append wouldn't
/// write again (finding 7 of slice 10's review): refused, and the save is
/// a Rewrite.
#[test]
fn append_rewrite_a_duplicate_s_button_action_follows_its_page() {
    let dir = scratch();
    let path = dir.join("a.pdf");
    let mut f = Form::new(2);
    let button = f.push_button("go", 0, crate::forms::fixture::row(0), "Go");
    let to = f.pages[1];
    let action = f.pdf.add_object(dictionary! {
        "S" => "GoTo", "D" => vec![to.into(), "Fit".into()],
    });
    f.dict(button).set("A", action);
    std::fs::write(&path, f.bytes()).unwrap();
    let mut doc = crate::open::file(&path, None).unwrap().doc;
    let [first, second] = [0, 1].map(|i| doc.page(i).unwrap());
    doc.apply(Edit::DuplicatePages { ids: vec![first] }, "duplicate")
        .unwrap();
    appends(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "the first duplicated");
    doc.apply(Edit::DuplicatePages { ids: vec![second] }, "duplicate")
        .unwrap();
    let report = planned(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "the second duplicated");
    assert_eq!(
        (report.strategy, report.why),
        (Some(StrategyKind::Rewrite), vec![Why::BaseObjectsMissing])
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A GoTo action to `to`, as an object of `f`.
fn go_to(f: &mut Form, to: ObjectId) -> ObjectId {
    f.pdf.add_object(dictionary! {
        "S" => "GoTo", "D" => vec![to.into(), "Fit".into()],
    })
}

/// A file of four pages, the last drawing nothing (a blank page, whose
/// object the removal check finds no text in) and named by what `holder`
/// puts in it, given the file and the last page's object. The last page
/// duplicated, copy 1 of it, and saved (`between`: else the two edits are
/// saved as one); then the last page itself deleted, which removes
/// nothing, as its duplicate draws what it drew, so the plan appends. But
/// no page is the last page's copy 0 now: what names it by number names a
/// page outside the tree, which a Rewrite prunes (slice 10's review, after
/// the commit). The Append is refused (`want`, given the document before
/// the delete), and the save is a Rewrite.
fn a_page_deleted_after_its_duplicate(
    holder: impl Fn(&mut Form, ObjectId),
    want: impl Fn(&Document) -> Refused,
) {
    for between in [true, false] {
        let dir = scratch();
        let path = dir.join("a.pdf");
        let mut f = Form::new(4);
        let to = f.pages[3];
        f.dict(to).remove(b"Contents");
        holder(&mut f, to);
        std::fs::write(&path, f.bytes()).unwrap();
        let mut doc = crate::open::file(&path, None).unwrap().doc;
        let want = want(&doc);
        let last = doc.page(3).unwrap();
        doc.apply(Edit::DuplicatePages { ids: vec![last] }, "duplicate")
            .unwrap();
        let copy = doc.page(4).unwrap();
        assert_eq!(doc.state().def(copy).unwrap().copy, 1);
        if between {
            appends(&mut doc, &path);
            reopens_as_rewritten(&doc, &dir, "duplicated");
        }
        doc.apply(Edit::DeletePages { ids: vec![last] }, "delete")
            .unwrap();
        assert_eq!(doc.state().def(copy).unwrap().copy, 1);
        let target = doc.save_target(&path);
        let strategy = plan(&doc.recovery_input(), &target).strategy;
        assert_eq!(
            strategy,
            Strategy::Append { onto: Onto::Saved },
            "{between}"
        );
        let refused = refusal(&doc);
        let report = planned(&mut doc, &path);
        reopens_as_rewritten(&doc, &dir, "deleted");
        assert_eq!(refused, Some(want), "{between}");
        assert_eq!(
            (report.strategy, report.why),
            (Some(StrategyKind::Rewrite), vec![Why::BaseObjectsMissing]),
            "{between}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The catalog's `/OpenAction` going to the last page.
#[test]
fn append_rewrite_an_open_action_to_a_page_deleted_after_its_duplicate_rewrites() {
    a_page_deleted_after_its_duplicate(
        |f, to| {
            let catalog = f.catalog;
            f.dict(catalog)
                .set("OpenAction", vec![to.into(), "Fit".into()]);
        },
        |_| Refused::Stale(ObjKey::Catalog),
    );
}

/// A named destination of the name dictionary's `/Dests` going to the
/// last page, and the structure tree's element on it.
#[test]
fn append_rewrite_names_of_a_page_deleted_after_its_duplicate_rewrite() {
    a_page_deleted_after_its_duplicate(
        |f, to| {
            let dests = f.pdf.add_object(dictionary! {
                "Names" => vec![Object::string_literal("last"), vec![to.into(), "Fit".into()].into()],
            });
            let catalog = f.catalog;
            f.dict(catalog)
                .set("Names", dictionary! { "Dests" => dests });
        },
        |_| Refused::Stale(ObjKey::Catalog),
    );
    a_page_deleted_after_its_duplicate(
        |f, to| {
            let element = f.pdf.add_object(dictionary! {
                "Type" => "StructElem", "S" => "P", "Pg" => to,
            });
            let root = f.pdf.add_object(dictionary! {
                "Type" => "StructTreeRoot", "K" => vec![element.into()],
            });
            let catalog = f.catalog;
            f.dict(catalog).set("StructTreeRoot", root);
        },
        |_| Refused::Stale(ObjKey::Catalog),
    );
}

/// The first page's own `/AA`, its open action going to the last page:
/// a page's own keys, read as copy 0.
#[test]
fn append_rewrite_a_page_s_action_to_a_page_deleted_after_its_duplicate_rewrites() {
    a_page_deleted_after_its_duplicate(
        |f, to| {
            let action = go_to(f, to);
            let first = f.pages[0];
            f.dict(first).set("AA", dictionary! { "O" => action });
        },
        |doc| Refused::Stale(ObjKey::Page(doc.page(0).unwrap())),
    );
}

/// A radio group on the first page, a field apart from its widgets, its
/// own `/AA` going to the last page: the fields' walk.
#[test]
fn append_rewrite_a_field_s_action_to_a_page_deleted_after_its_duplicate_rewrites() {
    use crate::forms::fixture::row;
    a_page_deleted_after_its_duplicate(
        |f, to| {
            let field = f.radio("pick", 0, &[row(0), row(1)], Some(0));
            let action = go_to(f, to);
            f.dict(field).set("AA", dictionary! { "Fo" => action });
        },
        |doc| {
            let st = doc.state();
            let (pick, _) = (st.fields()).find(|(_, n)| &*n.partial == "pick").unwrap();
            Refused::Stale(ObjKey::Field(pick))
        },
    );
}

/// A note of sheaf's on the first page, as a save writes it, with an
/// action going to the last page: a mark read from a file.
#[test]
fn append_rewrite_a_mark_s_action_to_a_page_deleted_after_its_duplicate_rewrites() {
    a_page_deleted_after_its_duplicate(
        |f, to| {
            let first = f.pages[0];
            let mut page = f.dict(first).clone();
            let marks = crate::forms::fixture::holding(&[crate::forms::fixture::note()]);
            crate::forms::fixture::write_marks(&mut f.pdf, &mut page, &marks);
            *f.dict(first) = page;
            let note = *f.annots(0).last().unwrap();
            let action = go_to(f, to);
            f.dict(note).set("A", action);
        },
        |doc| {
            let st = doc.state();
            let note = st.marks_on(doc.page(0).unwrap()).next().unwrap().id;
            Refused::Stale(ObjKey::Annot(note))
        },
    );
}
