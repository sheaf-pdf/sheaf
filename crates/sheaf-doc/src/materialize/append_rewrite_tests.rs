//! The Append ≡ Rewrite property (M10.5 §7.2, slice 10): after every save
//! in a random run of ops of every kind, transactions of several, groups,
//! undo, redo, jumps and history pruned to a budget ([`Run`]), with saves
//! as the plan decides, Save As, and Appends forced as "Append anyway"
//! forces them, the file written reopens as a fresh Rewrite of the same
//! state does. What's compared ([`Reopened`]): the pages in order, each
//! with its boxes, its content and resources by value, and its
//! annotations by name, with what they say and where links and widgets'
//! actions go, where each annotation that goes somewhere goes, the
//! records' or not, and which name another page as theirs; the outline;
//! the page labels; the field tree and values; the form's own keys and
//! whether it has an XFA part; the header and footer; the watermarks and
//! backgrounds found again; and where the open action goes. Half the runs
//! start from a file sheaf saved with a watermark and a background, so
//! what's found is set aside or kept as edits and saves go.
//!
//! Over ops since slice 11, with fixed cases beside it: labels moved with
//! their first page, links to pages a later Append brings, a node split and
//! then reordered, and the header and footer over a reorder, which stands
//! in for slice 17's live tokens. And a case for each gap the property
//! found: what a page shown as another page changes (its links, bookmarks
//! and number), fields parted from their widgets or written over them,
//! save policy's XFA drop applied and taken back, a header's pages and the
//! labels after a delete, a primary object's place taken by another record
//! of it, a comment made editable on a copy, and a page appended over a
//! primary page's object that page takes back. And for the gaps slice
//! 10's review found: a derived copy made the page's copy 0 by a move, and
//! a copy's action, inline link or screen annotation going to a page
//! brought in or duplicated after it. And for the rebase's: a screen
//! annotation on a page written at another number, or brought in again as
//! the copy an undone insert had, and a later instance's widget and field
//! read as the instance's copy. And for slice 12's: a connector group's
//! order after a page holding some of it turned, and saves after pruning
//! took the saved version, on the line and on an undone branch.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use in_progress::NoProgress;
use lopdf::{Dictionary, Document as Pdf, LazyPdf, Object, ObjectId, Stream, dictionary};
use proptest::prelude::*;
use sheaf_edit::{Anchor, Op, PageRef, Style as MarkStyle, Turn, Z};

use super::append::Refused;
use super::plan::{Onto, Strategy, forced_append};
use super::removal::Mode;
use super::removal_tests::scratch;
use super::{
    AskAnswers, How, ObjKey, SaveReport, StrategyKind, Why, materialize, plan, removal, save,
    write_file,
};
use crate::annot::{Annot, AnnotKind};
use crate::file::ForeignAnnot;
use crate::forms::fixture::{Form, every_kind, every_kind_in, outline_tree};
use crate::forms::{FieldId, FieldKind, Value};
use crate::geom::Geom;
use crate::history::{AllOf, FixedCount, MemoryBudget};
use crate::labels::{LabelRange, LabelSpec, Style};
use crate::model::history_tests::{self, Run, with_theirs};
use crate::model::{
    AnnotBody, DocState, Document, FieldProps, FormInst, ImageRef, PageForm, PageId, Scope, Target,
    TextStyle, WatermarkContent, XfaState,
};
use crate::stamps::HeaderFooter;
use crate::store::Src;
use crate::text_layer::{OcrWord, TextLayer};
use crate::tx::Derive;
use crate::tx::fixture as fx;

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
/// to each other, its push button going to its first page, with our marks
/// ([`history_tests::with_ours`]) and a screen annotation on its first
/// page ([`screen_on`]). The fixed cases' document, which has no open
/// action ([`files_opening`]).
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
    history_tests::with_ours(&mut other);
    screen_on(&mut other, 0);
    (f.bytes(), other.bytes())
}

/// A screen annotation on page `page` of `f`, which the records don't
/// hold, its `/P` the page.
fn screen_on(f: &mut Form, page: usize) {
    let p = f.pages[page];
    let screen = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Screen", "P" => p,
        "Rect" => vec![200.into(), 10.into(), 220.into(), 30.into()],
    });
    let mut annots = f.annots(page);
    annots.push(screen);
    f.set_annots(page, &annots);
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
    /// The subtypes of the entries of its `/Annots` whose `/P` is another
    /// page, in order.
    astray: Vec<String>,
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
    /// The watermarks and backgrounds found: each one's z, what it draws
    /// and where, what it replaces, and the places of the pages it's on
    /// (None: every page).
    watermarks: Vec<(Z, String, Option<Vec<usize>>)>,
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
            let subtype = |d: &Dictionary| {
                (d.get(b"Subtype").ok())
                    .and_then(|s| s.as_name().ok())
                    .map_or_else(String::new, |s| String::from_utf8_lossy(s).into_owned())
            };
            let entries =
                || (listed.iter()).filter_map(|e| pdf.dereference(e).ok()?.1.as_dict().ok());
            let goes = entries()
                .filter_map(|d| Some((subtype(d), dest_of(pdf, d, &ids)?)))
                .collect();
            let astray = entries()
                .filter(|d| {
                    d.get(b"P")
                        .is_ok_and(|p| p.as_reference().ok() != Some(facts.id))
                })
                .map(subtype)
                .collect();
            PageSays {
                geom: **geom,
                content,
                resources,
                annots,
                goes,
                astray,
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
    let watermarks = [Z::Under, Z::Over]
        .into_iter()
        .filter_map(|z| {
            let g = st.watermark(z)?;
            let wm = g.spec.watermark()?;
            // An image or a page by what it was drawn as, not where it is.
            let draws = match &wm.content {
                WatermarkContent::Image {
                    image: ImageRef::Placed(p),
                    scale,
                    opacity,
                } => format!("image {:?} {} {scale} {opacity}", p.res, p.turns),
                WatermarkContent::Page {
                    page: PageForm::Placed(p),
                    scale,
                    opacity,
                } => format!("page {:?} {scale} {opacity}", p.res),
                other => format!("{other:?}"),
            };
            let on = match g.scope().as_ref() {
                Scope::All => None,
                Scope::Pages(pages) => Some(places(st, &pages.iter().copied().collect::<Vec<_>>())),
            };
            let said = format!("{draws} {:?} {:?}", wm.position, wm.replaces);
            Some((z, said, on))
        })
        .collect();
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
        watermarks,
        open: opened_at(&opened.doc),
    }
}

/// Where `doc`'s file's catalog's `/OpenAction`, an explicit destination
/// or an action, goes among its pages.
fn opened_at(doc: &Document) -> Option<Dest> {
    let src = doc.primary();
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
        part(
            format!("page {i}'s annotations' other /P"),
            &a.astray,
            &b.astray,
        );
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
    part("watermarks".into(), &saved.watermarks, &fresh.watermarks);
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
/// [`history_tests::PRUNE`]: a save as planned to the document's file, Save
/// As to the other, and an Append forced onto either, each three times as
/// likely as one kind of edit.
const SAVES: u8 = history_tests::PRUNE + 1;
const ACTIONS: u8 = SAVES + 9;

/// `bytes` saved by sheaf with a watermark on every page and a background
/// on the second, which an open finds again: a file of `dir`'s.
fn watermarked(dir: &Path, bytes: Vec<u8>) -> Vec<u8> {
    let path = dir.join("marked.pdf");
    std::fs::write(&path, bytes).unwrap();
    let mut doc = crate::open::file(&path, None).unwrap().doc;
    let second = doc.page(1).unwrap();
    let ops = vec![
        fx::set_watermark(fx::watermark("DRAFT"), Z::Over, None),
        fx::set_watermark(fx::watermark("under"), Z::Under, Some(&[second])),
    ];
    doc.commit_ops(ops, "watermark").unwrap();
    How::Rewritten.write(&doc)
}

/// A run of `actions` over [`files`], with an XFA part when `xfa`, and
/// the document's file saved by sheaf with a watermark and a background
/// when `marked`: the saves compared as they're made.
fn run(xfa: bool, marked: bool, actions: &[(u8, u64)]) {
    // Mark ids come from the thread's generator: the same ones each run.
    fastrand::seed(1);
    let dir = scratch();
    let (primary, other) = files_opening(xfa, true);
    let primary = match marked {
        true => watermarked(&dir, primary),
        false => primary,
    };
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
    fn append_rewrite_reopen_alike(
        xfa in any::<bool>(),
        marked in any::<bool>(),
        actions in runs(),
    ) {
        run(xfa, marked, &actions);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    #[test]
    #[ignore = "1,024 cases: cargo test -p sheaf-doc --release -- --ignored append_rewrite"]
    fn append_rewrite_reopen_alike_1024(
        xfa in any::<bool>(),
        marked in any::<bool>(),
        actions in runs(),
    ) {
        run(xfa, marked, &actions);
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
    let labels = fx::labels(&[
        range(first, Style::LowerRoman),
        range(fourth, Style::Decimal),
    ]);
    doc.commit_ops(vec![labels], "labels").unwrap();
    appends(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "labelled");
    let labels = doc.state().page_labels().unwrap();
    assert_eq!(labels, ["i", "ii", "iii", "1", "2", "3"]);
    let first = fx::move_pages(&[fourth], Some(first));
    doc.commit_ops(vec![first], "move").unwrap();
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
    let (other, _) = doc.store().read_file(&b, None).unwrap();
    let links = |doc: &Document| -> usize {
        let st = doc.state();
        (st.annots())
            .filter(|(_, rec)| rec.foreign().is_some_and(|f| f.summary.is_link()))
            .count()
    };
    let before = links(&doc);
    for index in [0, 1] {
        let insert = fx::insert(&other, [index], "b.pdf", None);
        doc.commit_ops(vec![insert], "insert").unwrap();
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
    doc.commit_ops(vec![fx::duplicate(&[fifth])], "duplicate")
        .unwrap();
    appends(&mut doc, &path);
    assert!(doc.saved_record().unwrap().tree.nodes.len() > 1, "split");
    reopens_as_rewritten(&doc, &dir, "split");
    let [first, last] = [0, 130].map(|i| doc.page(i).unwrap());
    let reorder = fx::move_pages(&[last, fifth], Some(first));
    doc.commit_ops(vec![reorder], "reorder").unwrap();
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
    let set = fx::set_header(fx::header("Page <<1>> of <<N>>"), None);
    doc.commit_ops(vec![set], "header").unwrap();
    let report = save(&mut doc, &path, &answers(), &NoProgress).unwrap();
    assert_eq!(report.why, [Why::LegacyHeaderFooter]);
    reopens_as_rewritten(&doc, &dir, "header");
    let [first, last] = [0, 5].map(|i| doc.page(i).unwrap());
    let reorder = fx::move_pages(&[last], Some(first));
    doc.commit_ops(vec![reorder], "reorder").unwrap();
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

/// [`files`]' other file, a form, beside the document in `dir`, read
/// into `doc`'s store.
fn other(doc: &Document, dir: &Path) -> Src {
    let path = dir.join("form.pdf");
    std::fs::write(&path, files(false).1).unwrap();
    doc.store().read_file(&path, None).unwrap().0
}

/// Page `i` of `doc` shown as page `index` of `source`.
fn rebase(doc: &mut Document, i: usize, source: &Src, index: u32) {
    let id = doc.page(i).unwrap();
    let pages = vec![(id, source.clone(), index)];
    doc.commit_page_base(pages, Derive::StripHeaderFooter, "rebase")
        .unwrap();
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
    let primary = doc.primary().clone();
    rebase(&mut doc, 3, &primary, 5);
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
    let primary = doc.primary().clone();
    rebase(&mut doc, 3, &primary, 5);
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
    let other = other(&doc, &dir);
    let insert = || fx::insert(&other, [1], "form.pdf", None);
    doc.commit_ops(vec![insert()], "insert").unwrap();
    let copy = dir.join("b.pdf");
    assert_eq!(
        planned(&mut doc, &copy).strategy,
        Some(StrategyKind::Rewrite)
    );
    doc.commit_ops(vec![insert()], "insert").unwrap();
    appends(&mut doc, &copy);
    reopens_as_rewritten(&doc, &dir, "inserted again");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The other file's first page brought in twice, the first instance's
/// page deleted and saved, then the delete undone and saved again: each
/// instance's objects are the copy of its form it was made as (M10.5
/// §3.7), whatever comes or goes before it, so the file's objects of the
/// second stay its own and both saves append, reopening as a Rewrite
/// does. (Before slice 11 an instance's copy was its place among its
/// file's, which the first's coming back moved: the Append was refused.)
#[test]
fn append_rewrite_a_form_instance_keeps_its_copy_across_saves() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, false);
    let other = other(&doc, &dir);
    let insert = |name: &str| fx::insert(&other, [0], name, None);
    doc.commit_ops(vec![insert("form.pdf")], "insert").unwrap();
    doc.commit_ops(vec![insert("again.pdf")], "insert").unwrap();
    let copies = |doc: &Document| -> Vec<u32> {
        let st = doc.state();
        (st.pages().skip(6))
            .flat_map(|(page, _, _)| st.widgets_on(page).map(|(_, w)| w.inst.copy))
            .collect::<BTreeSet<u32>>()
            .into_iter()
            .collect()
    };
    assert_eq!(copies(&doc), [0, 1]);
    let first = doc.page(6).unwrap();
    doc.commit_ops(vec![fx::delete(&[first])], "delete")
        .unwrap();
    appends(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "the first deleted");
    doc.undo().unwrap();
    assert_eq!(copies(&doc), [0, 1]);
    appends(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "the first back");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The other file's pages brought in and saved, the insert undone, and
/// the same bytes brought in again: the new instance takes the copy the
/// undone one had, the next its source has in the state, so its form's
/// objects are the ones the file has of that copy. The path removes the
/// first insert's comments and fields, so the plan rewrites; appended
/// anyway, the section that writes the new fields with them reopens as a
/// Rewrite does. The first page's screen, which the records don't hold,
/// is written again with it: the file has it with the undone insert's
/// page as its `/P`, which reaches that page's widgets, on no page now.
#[test]
fn append_rewrite_a_reused_form_copy_appends() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, false);
    let other = other(&doc, &dir);
    let insert = fx::insert(&other, [0, 1], "form.pdf", None);
    let inst = FormInst {
        src: other.id(),
        copy: 0,
    };
    let fields = |doc: &Document| -> BTreeSet<FieldId> {
        let st = doc.state();
        (st.fields())
            .filter(|(_, n)| n.inst == inst)
            .map(|(id, _)| id)
            .collect()
    };
    doc.commit_ops(vec![insert.clone()], "insert").unwrap();
    appends(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "inserted");
    let was = fields(&doc);
    doc.undo().unwrap();
    doc.commit_ops(vec![insert], "insert again").unwrap();
    // The same copy, fields of new ids.
    let now = fields(&doc);
    assert!(!now.is_empty() && was.is_disjoint(&now), "{was:?} {now:?}");
    let v = doc.current_version();
    let input = doc.save_input(v).unwrap();
    let planned = plan(&input, &doc.save_target(&path)).strategy;
    doc.abandon_save(v);
    let removes = |w: &Why| matches!(w, Why::Removes(_));
    assert!(
        matches!(&planned, Strategy::Rewrite { why } if why.iter().all(removes)),
        "{planned:?}"
    );
    assert_eq!(forced(&mut doc, &path), APPENDED);
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
    let other = other(&doc, &dir);
    rebase(&mut doc, 1, &other, 1);
    let copy = dir.join("b.pdf");
    planned(&mut doc, &copy);
    let first = doc.page(0).unwrap();
    doc.commit_ops(vec![fx::duplicate(&[first, first])], "duplicate")
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
    doc.commit_ops(vec![fx::delete(&[last])], "delete").unwrap();
    let other = other(&doc, &dir);
    let insert = fx::insert(&other, [0], "form.pdf", None);
    doc.commit_ops(vec![insert], "insert").unwrap();
    assert_eq!(
        planned(&mut doc, &path).strategy,
        Some(StrategyKind::Rewrite)
    );
    let first = doc.page(0).unwrap();
    doc.commit_ops(vec![fx::duplicate(&[first])], "duplicate")
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
    doc.commit_ops(vec![fx::delete(&[second])], "delete")
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
    let set = fx::set_header(fx::header("On one page"), Some(&[fifth]));
    doc.commit_ops(vec![set], "header").unwrap();
    assert_eq!(planned(&mut doc, &path).why, [Why::LegacyHeaderFooter]);
    let first = doc.page(0).unwrap();
    doc.commit_ops(vec![fx::delete(&[first])], "delete")
        .unwrap();
    assert_eq!(forced(&mut doc, &path), APPENDED);
    reopens_as_rewritten(&doc, &dir, "deleted");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The page labels' ranges starting at the third page, the fifth and the
/// sixth, appended; the third and fifth deleted: the first range starts at
/// the fourth, the next page of its range (M10.5 proposal 25), and the
/// second, with no page of it left, goes, and the catalog, written again,
/// says so. (Not the sixth: the push button's action goes to it, and an
/// Append that deletes it is refused, the next case.)
#[test]
fn append_rewrite_labels_start_at_the_next_page_of_their_range() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, false);
    let [third, fifth, sixth] = [2, 4, 5].map(|i| doc.page(i).unwrap());
    let range = |start, prefix: &str| LabelSpec {
        start,
        style: Style::Decimal,
        prefix: prefix.into(),
        first: 1,
    };
    let ranges = [range(third, "A-"), range(fifth, "B-"), range(sixth, "C-")];
    doc.commit_ops(vec![fx::labels(&ranges)], "labels").unwrap();
    appends(&mut doc, &path);
    let delete = fx::delete(&[third, fifth]);
    doc.commit_ops(vec![delete], "delete").unwrap();
    assert_eq!(forced(&mut doc, &path), APPENDED);
    reopens_as_rewritten(&doc, &dir, "deleted");
    let back = crate::open::file(&path, None).unwrap().doc;
    let starts: Vec<(u32, String)> = (back.state().label_ranges().unwrap().into_iter())
        .map(|r| (r.start, r.prefix))
        .collect();
    let want = [(0, String::new()), (2, "A-".into()), (3, "C-".into())];
    assert_eq!(starts, want);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The page labels' ranges starting at the third page and the sixth, the
/// last, appended; those pages deleted: the push button's action goes to
/// the sixth, which an Append would leave naming a page outside the tree,
/// so it's refused (slice 10's review) and the save is a Rewrite. The
/// first range starts at the fourth, and the second, with no page of it
/// left, goes.
#[test]
fn append_rewrite_labels_of_the_last_page_go_with_it() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, false);
    let [third, sixth] = [2, 5].map(|i| doc.page(i).unwrap());
    let range = |start, prefix: &str| LabelSpec {
        start,
        style: Style::Decimal,
        prefix: prefix.into(),
        first: 1,
    };
    let labels = fx::labels(&[range(third, "A-"), range(sixth, "B-")]);
    doc.commit_ops(vec![labels], "labels").unwrap();
    appends(&mut doc, &path);
    let delete = fx::delete(&[third, sixth]);
    doc.commit_ops(vec![delete], "delete").unwrap();
    let st = doc.state();
    let (print, _) = (st.fields())
        .find(|(_, n)| &*n.partial == "print[0]")
        .unwrap();
    let (button, _) = (st.widgets_on(doc.page(1).unwrap()))
        .find(|(_, w)| w.field == print)
        .unwrap();
    assert_eq!(refusal(&doc), Some(Refused::Stale(ObjKey::Annot(button))));
    let why = vec![Why::BaseObjectsMissing];
    assert_eq!(forced(&mut doc, &path), Strategy::Rewrite { why });
    reopens_as_rewritten(&doc, &dir, "deleted");
    let back = crate::open::file(&path, None).unwrap().doc;
    let starts: Vec<(u32, String)> = (back.state().label_ranges().unwrap().into_iter())
        .map(|r| (r.start, r.prefix))
        .collect();
    assert_eq!(starts, [(0, String::new()), (2, "A-".into())]);
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
    let other = other(&doc, &dir);
    let copy = dir.join("b.pdf");
    planned(&mut doc, &copy);
    rebase(&mut doc, 4, &other, 0);
    let third = doc.page(2).unwrap();
    let primary = doc.primary().clone();
    let replace = fx::replace(&[third], &primary, 3..5, "a.pdf");
    doc.commit_ops(vec![replace], "replace").unwrap();
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
/// widgets are an instance of the form of their own (M10.5 §3.7), written
/// as objects of their own, so nothing the section refers to is the
/// primary's objects the fifth page's widgets were in the file, and the
/// Append is taken.
#[test]
fn append_rewrite_a_page_replaced_by_its_own_page_is_a_form_of_its_own() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, false);
    let other = other(&doc, &dir);
    let [first, fifth] = [0, 4].map(|i| doc.page(i).unwrap());
    let primary = doc.primary().clone();
    let pages = vec![(fifth, primary.clone(), 0), (first, other, 0)];
    doc.commit_page_base(pages, Derive::StripHeaderFooter, "rebase")
        .unwrap();
    let copy = dir.join("b.pdf");
    planned(&mut doc, &copy);
    let replace = fx::replace(&[first], &primary, [0], "a.pdf");
    doc.commit_ops(vec![replace], "replace").unwrap();
    let st = doc.state();
    let copies: BTreeSet<u32> = (st.widgets_on(first)).map(|(_, w)| w.inst.copy).collect();
    assert_eq!(copies, BTreeSet::from([1]));
    assert_eq!(forced(&mut doc, &copy), APPENDED);
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
    let primary = doc.primary().clone();
    let insert = fx::insert(&primary, [1], "a.pdf", None);
    doc.commit_ops(vec![insert], "insert").unwrap();
    appends(&mut doc, &path);
    let copy = doc.page(6).unwrap();
    let id = (doc.state().foreign_on(copy))
        .find(|(_, f)| f.summary.name == "theirs-2-reply")
        .map(|(id, _)| id)
        .unwrap();
    assert!(doc.state().annot(id).unwrap().foreign().unwrap().copy > 0);
    doc.commit_ops(vec![fx::adopt(&[id])], "edit").unwrap();
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

/// Another tool's replacement on the first page, its StrikeOut deleted and
/// appended anyway; then its Caret made editable, which keeps its place and is a
/// replacement written with a StrikeOut of its own: the Append writes the
/// page, whose `/Annots` lists the StrikeOut beside it, as a Rewrite does.
#[test]
fn append_rewrite_a_replacement_made_editable_lists_its_strikeout() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, false);
    let first = doc.page(0).unwrap();
    let theirs = |doc: &Document, name: &str| {
        (doc.state().foreign_on(first))
            .find(|(_, f)| f.summary.name == name)
            .map(|(id, f)| (id, f.summary.clone()))
            .unwrap()
    };
    let (strike, _) = theirs(&doc, "theirs-1-strike");
    let delete = fx::delete_annots(&[strike]);
    doc.commit_ops(vec![delete], "delete").unwrap();
    assert_eq!(forced(&mut doc, &path), APPENDED);
    let (id, _) = theirs(&doc, "theirs-1");
    doc.commit_ops(vec![fx::adopt(&[id])], "edit").unwrap();
    let made = doc.state().annot(id).unwrap().mark().unwrap();
    assert!(matches!(made.kind, AnnotKind::Replace { .. }));
    appends(&mut doc, &path);
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
    let primary = doc.primary().clone();
    rebase(&mut doc, 0, &primary, 2);
    rebase(&mut doc, 1, &primary, 0);
    rebase(&mut doc, 0, &primary, 1);
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
    let (other, _) = doc.store().read_file(&b, None).unwrap();
    for pages in [vec![0, 1], vec![0], vec![1]] {
        let insert = fx::insert(&other, pages, "b.pdf", None);
        doc.commit_ops(vec![insert], "insert").unwrap();
        appends(&mut doc, &path);
        reopens_as_rewritten(&doc, &dir, "inserted");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file of two pages, the first named by what `holder` puts in it,
/// given the file and its first page's object. The first page duplicated,
/// and both shown from one derived copy (cropped in it), whose two pages
/// are both made from the first page: the first in order is its copy 0,
/// the page its object is, and the duplicate copy 1. Saved; then the duplicate moved first, which
/// makes it copy 0 and the first copy 1, though neither is written (one
/// node holds both). The file's number for the first page's object still
/// holds the first, where what names it by number goes, while a Rewrite
/// writes the duplicate there: the Append is refused (finding 6 of slice
/// 10's review), and the save is a Rewrite.
fn a_derived_copy_made_copy_0_by_a_move(
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
    doc.commit_ops(vec![fx::duplicate(&[first])], "duplicate")
        .unwrap();
    let copy = doc.page(1).unwrap();
    assert_eq!(doc.state().def(copy).unwrap().copy, 1);
    let primary = doc.primary().clone();
    let bytes = crate::save::derived_copy(&primary, &[0, 0], &mut |_, page, _| {
        let crop = [0.0f32, 0.0, 400.0, 500.0].map(Object::from);
        page.set("CropBox", crop.to_vec());
        Ok(())
    })
    .unwrap();
    let kind = BlobKind::Pdf {
        password: None,
        origin: Origin::Memory(MemoryKind::Derived),
    };
    let copied = doc.store().open(Arc::from(bytes), kind).unwrap();
    let derived = crate::Derived {
        from: primary,
        pages: vec![0, 0],
    };
    doc.add_derived(&copied, derived);
    let pages = vec![(first, copied.clone(), 0), (copy, copied, 1)];
    let strip = Derive::StripHeaderFooter;
    doc.commit_page_base(pages, strip, "strip").unwrap();
    planned(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "copied");
    doc.commit_ops(vec![fx::move_pages(&[copy], Some(first))], "move")
        .unwrap();
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
fn append_rewrite_a_derived_copy_made_copy_0_by_a_move_rewrites() {
    a_derived_copy_made_copy_0_by_a_move(
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
fn append_rewrite_an_open_action_to_a_derived_copy_made_copy_0_rewrites() {
    a_derived_copy_made_copy_0_by_a_move(
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
/// brought in, then each page of `again` again, each saved so. The first
/// page again: its copy of the action goes to the second page's only
/// copy. Then the second page again: when the copy's action is read as
/// the page's copy, it goes to that copy now, which the Append wouldn't
/// write again (finding 7 of slice 10's review): `refused`, and the last
/// save is a Rewrite. Else it's an Append.
fn a_copy_s_action_follows_its_page(
    holder: impl FnOnce(&mut Form, ObjectId),
    again: &[u32],
    refused: bool,
) {
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
    let (other, _) = doc.store().read_file(&b, None).unwrap();
    let inserts = std::iter::once(vec![0, 1]).chain(again.iter().map(|&p| vec![p]));
    for (i, pages) in inserts.enumerate() {
        let insert = fx::insert(&other, pages, "b.pdf", None);
        doc.commit_ops(vec![insert], "insert").unwrap();
        let report = planned(&mut doc, &path);
        reopens_as_rewritten(&doc, &dir, &format!("insert {i}"));
        if i == again.len() {
            let want = match refused {
                true => (Some(StrategyKind::Rewrite), vec![Why::BaseObjectsMissing]),
                false => (Some(StrategyKind::Append), vec![]),
            };
            assert_eq!((report.strategy, report.why), want);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A push button on the other file's first page, its action going to
/// the second.
fn button(f: &mut Form, action: ObjectId) {
    let button = f.push_button("go", 0, crate::forms::fixture::row(0), "Go");
    f.dict(button).set("A", action);
}

/// With [`button`], the first page brought in again and then the second:
/// each insert is an instance of the form of its own (M10.5 §3.7), so the
/// first page's copy's button is the second instance's, read as the
/// page's copy 1, and its action goes to the second page's copy 1, which
/// the third insert brings: refused. (Before slice 11 the pages brought in
/// again joined the first insert's instance, whose button, copy 0, went to
/// the second page's copy 0 still: an Append.)
#[test]
fn append_rewrite_a_copy_s_button_action_follows_its_page() {
    a_copy_s_action_follows_its_page(button, &[0, 1], true);
}

/// With [`button`], only the second page brought in again: the first
/// instance's button is copy 0's, and its action goes to the second
/// page's copy 0 still, which both write: an Append.
#[test]
fn append_rewrite_a_copy_s_button_of_the_first_instance_appends() {
    a_copy_s_action_follows_its_page(button, &[1], false);
}

/// With what `holder` puts on the other file's first page and a text field
/// on its second, the second page brought in again, then the first, then
/// the second: each insert is an instance of the form of its own, the
/// second's with the text field, so the first page's copy 1 is the third
/// instance, copy 2 of the form, and what it holds goes to the second
/// page's copy 2, which the last insert brings: refused. Read as its
/// page's copy, it would go to the second insert's page still, which the
/// Append writes as it did. Widgets and fields are walked as their
/// instance's copy (`Closure::stale`), as the emitter writes them.
fn a_later_instance_s_action_follows_its_page(holder: impl FnOnce(&mut Form, ObjectId)) {
    a_copy_s_action_follows_its_page(
        |f, action| {
            holder(f, action);
            f.text("t", 1, crate::forms::fixture::row(0));
        },
        &[1, 0, 1],
        true,
    );
}

/// [`button`], one object with its field: both walks.
#[test]
fn append_rewrite_a_later_instance_s_button_follows_its_page() {
    a_later_instance_s_action_follows_its_page(button);
}

/// A radio button's widget, apart from its field, with the action: the
/// widgets' walk.
#[test]
fn append_rewrite_a_later_instance_s_widget_action_follows_its_page() {
    use crate::forms::fixture::row;
    a_later_instance_s_action_follows_its_page(|f, action| {
        let field = f.radio("pick", 0, &[row(1), row(2)], Some(0));
        let widget = f.widgets_of(field)[0];
        f.dict(widget).set("A", action);
    });
}

/// A radio group, its own `/AA` the action: the fields' walk.
#[test]
fn append_rewrite_a_later_instance_s_field_action_follows_its_page() {
    use crate::forms::fixture::row;
    a_later_instance_s_action_follows_its_page(|f, action| {
        let field = f.radio("pick", 0, &[row(1), row(2)], Some(0));
        f.dict(field).set("AA", dictionary! { "Fo" => action });
    });
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
        &[0, 1],
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
        &[0, 1],
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
    doc.commit_ops(vec![fx::duplicate(&[first])], "duplicate")
        .unwrap();
    appends(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "the first duplicated");
    doc.commit_ops(vec![fx::duplicate(&[second])], "duplicate")
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
        doc.commit_ops(vec![fx::duplicate(&[last])], "duplicate")
            .unwrap();
        let copy = doc.page(4).unwrap();
        assert_eq!(doc.state().def(copy).unwrap().copy, 1);
        if between {
            appends(&mut doc, &path);
            reopens_as_rewritten(&doc, &dir, "duplicated");
        }
        doc.commit_ops(vec![fx::delete(&[last])], "delete").unwrap();
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

/// Another tool's reply made editable and appended, then made theirs
/// again by undo, and saved as another file: the reply says its comment's
/// name as the mark said it, so nothing it said is gone, and the removal
/// check, as the Rewrite's oracle, finds nothing.
#[test]
fn append_rewrite_a_reply_made_editable_then_undone_removes_nothing() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, false);
    let first = doc.page(0).unwrap();
    let reply = (doc.state().foreign_on(first))
        .find(|(_, f)| f.summary.name == "theirs-1-reply")
        .map(|(id, _)| id)
        .unwrap();
    doc.commit_ops(vec![fx::adopt(&[reply])], "edit").unwrap();
    appends(&mut doc, &path);
    doc.undo().unwrap();
    let report = planned(&mut doc, &dir.join("b.pdf"));
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    reopens_as_rewritten(&doc, &dir, "made theirs again");
    let _ = std::fs::remove_dir_all(&dir);
}

/// After a Rewrite to another file, the fifth page replaced by the other
/// file's second, which a forced Append writes over the object the fifth
/// page was; then the primary's fourth and fifth pages brought in again,
/// before it when `before`, else at the end, copy 0 of the fifth free
/// again, and its first page too when `first`: the page that's that copy
/// takes its object's number back, so the replacement is written at a
/// number of its own, with what names it by number: its widgets' `/P`,
/// which the form's check finds wrong in the file else. The save
/// appends, and reopens as a Rewrite does. With `screen`, the replacement
/// is the other file's first page, with its screen annotation, which the
/// records don't hold: the file has it as the forced Append wrote it, its
/// `/P` the number the page leaves, so it's written again too.
fn a_page_appended_over_a_primary_page(before: bool, first: bool, screen: bool) {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, false);
    let other = other(&doc, &dir);
    let copy = dir.join("b.pdf");
    assert_eq!(planned(&mut doc, &copy).why, [Why::OtherTarget]);
    let fifth = doc.page(4).unwrap();
    let index = if screen { 0 } else { 1 };
    let replace = fx::replace(&[fifth], &other, [index], "form.pdf");
    doc.commit_ops(vec![replace], "replace").unwrap();
    assert_eq!(forced(&mut doc, &copy), APPENDED);
    reopens_as_rewritten(&doc, &dir, "replaced");
    let was = doc.saved_record().unwrap().map.keys[&ObjKey::Page(fifth)];
    let primary = doc.primary().clone();
    let at = before.then_some(fifth);
    let insert = fx::insert(&primary, [3, 4], "a.pdf", at);
    doc.commit_ops(vec![insert], "insert").unwrap();
    if first {
        let insert = fx::insert(&primary, [0], "a.pdf", None);
        doc.commit_ops(vec![insert], "insert").unwrap();
    }
    appends(&mut doc, &copy);
    reopens_as_rewritten(&doc, &dir, "brought in again");
    let map = &doc.saved_record().unwrap().map;
    assert_ne!(map.keys[&ObjKey::Page(fifth)], was);
    let bytes = std::fs::read(&copy).unwrap();
    let findings = crate::forms::check_latest(&bytes, None).unwrap().findings;
    let wrong = |f: &&crate::forms::Finding| matches!(f, crate::forms::Finding::WrongPage(_));
    assert_eq!(findings.iter().find(wrong), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn append_rewrite_a_page_appended_over_a_primary_page_gives_its_number_back() {
    a_page_appended_over_a_primary_page(true, true, false);
}

#[test]
fn append_rewrite_a_page_appended_over_a_primary_page_writes_its_widgets_again() {
    a_page_appended_over_a_primary_page(true, false, false);
}

#[test]
fn append_rewrite_a_page_appended_over_a_primary_page_gives_it_up_wherever_it_comes() {
    a_page_appended_over_a_primary_page(false, true, false);
}

#[test]
fn append_rewrite_a_page_appended_over_a_primary_page_writes_its_screen_again() {
    a_page_appended_over_a_primary_page(true, false, true);
}

#[test]
fn append_rewrite_a_page_appended_over_a_primary_page_writes_its_screen_again_wherever_it_comes() {
    a_page_appended_over_a_primary_page(false, true, true);
}

/// A connector pair on the second page, and a copy of that page, whose
/// copy of the pair is the same group: each connector goes to the next of
/// the group in the order they show in. The copy turned upside down shows
/// its pair the other way round, so the next of the first page's second
/// connector is the copy's other one: written again, as a Rewrite writes
/// it.
#[test]
fn append_rewrite_a_connector_s_next_follows_a_turned_page() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, false);
    let second = doc.page(1).unwrap();
    let at = |x| Anchor {
        page: PageRef::Id(second),
        rect: crate::annot::Box2 {
            x,
            y: 40.0,
            w: 22.0,
            h: 22.0,
        },
    };
    let pair = Op::AddConnector {
        a: at(40.0),
        b: at(80.0),
        label: "1".into(),
        style: MarkStyle {
            color: [255, 0, 0],
            opacity: 1.0,
            stroke: 1.0,
            fill: None,
            dashed: false,
            cloudy: false,
        },
        locals: Vec::new(),
    };
    doc.commit_ops(vec![pair, fx::duplicate(&[second])], "pair")
        .unwrap();
    appends(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "copied");
    let copy = doc.page(2).unwrap();
    doc.commit_ops(vec![fx::rotate(&[copy], Turn::Half)], "turn")
        .unwrap();
    appends(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "turned");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A group of 2,000 connectors, then every page turned: each appended as
/// a Rewrite writes it, on a stack of 1 MiB. Each connector's next is
/// another of the group, which the closure takes once for the group: one
/// taken again for each connector it takes recursed as deep as the group
/// is long, and for each page turned scanned every annotation again.
#[test]
fn append_rewrite_a_long_connector_group_is_taken_once() {
    let saves = std::thread::Builder::new().stack_size(1 << 20).spawn(|| {
        let dir = scratch();
        let (path, mut doc) = opened(&dir, false);
        let pages: Vec<PageId> = doc.state().order().iter().copied().collect();
        let at = |i: usize| Anchor {
            page: PageRef::Id(pages[i % pages.len()]),
            rect: crate::annot::Box2 {
                x: (i % 20) as f32 * 25.0,
                y: (i / 20 % 30) as f32 * 25.0,
                w: 22.0,
                h: 22.0,
            },
        };
        let pair = |i: usize| Op::AddConnector {
            a: at(2 * i),
            b: at(2 * i + 1),
            label: "1".into(),
            style: MarkStyle {
                color: [255, 0, 0],
                opacity: 1.0,
                stroke: 1.0,
                fill: None,
                dashed: false,
                cloudy: false,
            },
            locals: Vec::new(),
        };
        doc.commit_ops((0..1000).map(pair).collect(), "pairs")
            .unwrap();
        appends(&mut doc, &path);
        reopens_as_rewritten(&doc, &dir, "connected");
        doc.commit_ops(vec![fx::rotate(&pages, Turn::Quarter)], "turn")
            .unwrap();
        appends(&mut doc, &path);
        reopens_as_rewritten(&doc, &dir, "turned");
        let _ = std::fs::remove_dir_all(&dir);
    });
    saves.unwrap().join().unwrap();
}

/// A text layer of one word, `text`, as OCR recognizes it on a page.
fn layer(text: &str) -> Arc<TextLayer> {
    let word = OcrWord {
        text: text.into(),
        rect: crate::annot::Box2 {
            x: 72.0,
            y: 700.0,
            w: 60.0,
            h: 12.0,
        },
        confidence: 1.0,
    };
    let geom = Geom::read(None, None, 0);
    Arc::new(crate::text_layer::from_engine(vec![word], &geom, "test"))
}

/// `doc`'s history kept from now on to `budget` bytes, one snapshot kept.
fn budget(doc: &mut Document, budget: usize) {
    let policy = AllOf(vec![
        Box::new(FixedCount(1)),
        Box::new(MemoryBudget(budget)),
    ]);
    doc.set_policy(Box::new(policy));
}

/// A save, then edits under a budget so small that the first version
/// kept moves past the saved one (M10.5 §8.7): what's kept of it still
/// says what the file holds and what changed since, so the next save
/// appends, its path keys every slot the edits since changed, and the
/// file reopens as a fresh Rewrite does.
#[test]
fn append_rewrite_a_save_after_the_root_passes_the_saved_version_appends() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, false);
    let pages: Vec<PageId> = doc.state().order().iter().copied().collect();
    doc.commit_ops(vec![fx::rotate(&pages[..1], Turn::Quarter)], "turn")
        .unwrap();
    appends(&mut doc, &path);
    let saved = doc.current_version();
    budget(&mut doc, 1);
    let edits = [
        vec![fx::rotate(&pages[1..2], Turn::Quarter)],
        fx::ocr([(pages[2], layer("after"))]),
        vec![fx::rotate(&pages[..1], Turn::Half)],
    ];
    let mut changed = BTreeSet::new();
    for ops in edits {
        let commit = doc.commit_ops(ops, "edit").unwrap();
        changed.extend(commit.change.keys.iter().copied());
    }
    assert!(doc.jump(saved).is_err(), "pruned");
    assert!(doc.is_dirty() && !doc.can_undo());
    let v = doc.current_version();
    let input = doc.save_input(v).unwrap();
    doc.abandon_save(v);
    assert_eq!(*input.path_keys, changed);
    appends(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "the saved version pruned");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A save, undone, then OCR runs over every page under a small budget:
/// the branch holding the saved version goes once the first version kept
/// moves past where it turns off, the saved version detaching, and what
/// history holds stays within the budget. The next save appends, turning
/// back the page the saved version turned, and reopens as a fresh Rewrite
/// does.
#[test]
fn append_rewrite_a_saved_version_on_an_undone_branch_detaches() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, false);
    let pages: Vec<PageId> = doc.state().order().iter().copied().collect();
    doc.commit_ops(vec![fx::rotate(&pages[..1], Turn::Quarter)], "turn")
        .unwrap();
    appends(&mut doc, &path);
    let saved = doc.current_version();
    doc.undo().unwrap();
    let ocr = |doc: &mut Document, run: usize| {
        let layers = (pages.iter()).map(|&p| (p, layer(&format!("run {run}"))));
        doc.commit_ops(fx::ocr(layers), "OCR").unwrap();
    };
    ocr(&mut doc, 0);
    // Room for about three runs.
    let room = 3 * doc.stats().total_bytes();
    budget(&mut doc, room);
    for run in 1..20 {
        ocr(&mut doc, run);
        let held = doc.stats().total_bytes();
        assert!(held <= room, "run {run}: {held} bytes of history");
    }
    assert!(doc.jump(saved).is_err(), "pruned");
    assert!(doc.version_count() < 10 && doc.is_dirty());
    appends(&mut doc, &path);
    reopens_as_rewritten(&doc, &dir, "the saved version's branch pruned");
    let _ = std::fs::remove_dir_all(&dir);
}
