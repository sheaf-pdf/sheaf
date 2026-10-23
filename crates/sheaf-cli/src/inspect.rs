//! `sheaf inspect --json`: what a PDF holds, as `sheaf edit`'s ops name it
//! (M10.5 §14.2), for scripts that write edits: its pages by position,
//! with their labels, boxes and turn; sheaf's marks and other tools'
//! comments by `/NM`; its bookmarks by their titles; its headers, footers,
//! watermarks and backgrounds; and a summary of its form.

use std::path::PathBuf;

use clap::Args;
use serde_json::{Value, json};
use sheaf_doc::geom::Rect;
use sheaf_doc::model::Scope;
use sheaf_doc::{DocState, GenSpec};
use sheaf_edit::{RangeSet, Z};
use sheaf_ops::session::Session;

use crate::names::Names;

#[derive(Args)]
#[command(about = "Print what a PDF holds, as `sheaf edit` names it")]
pub struct InspectArgs {
    /// The PDF to inspect.
    input: PathBuf,
    #[arg(long)]
    password: Option<String>,
    /// Print it as JSON (the only form so far).
    #[arg(long, required = true)]
    json: bool,
}

pub fn run(args: InspectArgs, quiet: bool) -> anyhow::Result<()> {
    let session = crate::progress::run("opening", quiet, |p| {
        Session::open(&args.input, args.password.as_deref(), p)
    })?;
    crate::print_json(inspect(&session), true);
    Ok(())
}

/// What `session`'s document holds, in the shape docs/document-model.md
/// gives.
fn inspect(session: &Session) -> Value {
    let st = session.doc.state();
    let names = Names::of(st);
    let pages: Vec<Value> = (st.pages().enumerate())
        .map(|(i, (id, _, geom))| {
            let mut boxes = json!({ "media": corners(geom.media), "crop": corners(geom.crop) });
            for (name, rect) in [
                ("trim", geom.trim),
                ("bleed", geom.bleed),
                ("art", geom.art),
            ] {
                if let Some(rect) = rect {
                    boxes[name] = corners(rect);
                }
            }
            json!({
                "pos": i + 1,
                "label": st.label_of(id).as_deref(),
                "boxes": boxes,
                "rotate": geom.rotate.degrees(),
            })
        })
        .collect();
    // By page, each as its page lists them.
    let (mut marks, mut comments) = (Vec::new(), Vec::new());
    for (i, (id, _, _)) in st.pages().enumerate() {
        for (_, rec) in st.annots_on(id) {
            if let Some(mark) = rec.mark() {
                let kind = serde_json::to_value(sheaf_doc::tx::mark_kind(&mark.kind));
                let kind = kind.map_or(Value::Null, |k| k["type"].clone());
                marks.push(json!({ "nm": rec.name(), "page": i + 1, "kind": kind }));
            } else if let Some(f) = rec.foreign().filter(|f| !f.summary.is_link()) {
                let subtype = &f.summary.subtype;
                comments.push(json!({ "nm": rec.name(), "page": i + 1, "subtype": subtype }));
            }
        }
    }
    let bookmarks: Vec<Value> = (st.bookmarks_preorder().into_iter())
        .map(|(_, id, b)| {
            let page = b.page().and_then(|p| names.page(p));
            json!({ "path": names.bookmark(id), "page": page })
        })
        .collect();
    // One of each kind at most, by kind: their ids are random.
    let mut generators: Vec<(&str, Value)> = (st.generators().values())
        .map(|g| {
            let kind = match (&g.spec, g.z) {
                (GenSpec::HeaderFooter(_), _) => "header-footer",
                (GenSpec::Watermark(_), Z::Over) => "watermark",
                (GenSpec::Watermark(_), Z::Under) => "background",
            };
            (
                kind,
                json!({ "kind": kind, "scope": scope(&names, &g.scope) }),
            )
        })
        .collect();
    generators.sort_by_key(|&(kind, _)| kind);
    let generators: Vec<Value> = generators.into_iter().map(|(_, g)| g).collect();
    json!({
        "pages": pages,
        "marks": marks,
        "comments": comments,
        "bookmarks": bookmarks,
        "generators": generators,
        "form": form(session, st),
    })
}

/// A box as `[x0, y0, x1, y1]`, in points.
fn corners(r: Rect) -> Value {
    json!([r.x0, r.y0, r.x1, r.y1])
}

/// The pages a generator is on: `"all"`, those added later too, or their
/// positions, as `"1-3,5"`.
fn scope(names: &Names, scope: &Scope) -> Value {
    match scope {
        Scope::All => json!("all"),
        Scope::Pages(pages) => {
            let runs = (pages.iter()).filter_map(|&p| names.page(p)).map(|pos| {
                let pos = pos as u32;
                pos..=pos
            });
            json!(RangeSet::from_runs(runs).map(|set| set.to_string()).ok())
        }
    }
}

/// The form: its fields (those holding a value) and their widgets on
/// pages, its XFA part (none, static, or dynamic: the XFA is the form),
/// and the file's signatures, and of them those whose signed bytes
/// changed (their digests checked, `Signature::integrity`).
fn form(session: &Session, st: &DocState) -> Value {
    let form = st.form();
    let widgets: usize = st.terminals().map(|(_, _, t)| t.widgets.len()).sum();
    let signatures = session.doc.primary().signatures();
    let xfa = match (form.xfa.present, form.xfa.dynamic) {
        (false, _) => "none",
        (true, false) => "static",
        (true, true) => "dynamic",
    };
    json!({
        "fields": st.terminals().count(),
        "widgets": widgets,
        "xfa": xfa,
        "signatures": signatures.signed.len(),
        "signatures_broken": signatures.signed.iter().filter(|s| s.integrity().is_broken()).count(),
    })
}
