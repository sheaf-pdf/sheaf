//! Exports of a [`Document`](crate::Document) snapshot, until slice 19's
//! `export::run`. A save is [`materialize::save`]'s, or the app's through
//! [`materialize::write_file`].
//!
//! Every file a snapshot is written as whole is the new writer's
//! ([`materialize`]): a Rewrite over the primary's objects, as save
//! policy's form rules plan it ([`crate::forms::rules::plan`]). Saves
//! keep the primary's encryption and object numbers; exports keep its
//! encryption (M10.5 proposal 10; print doesn't) and number from 1
//! ([`Profile`]). An export writes what [`subset`] keeps of the state,
//! markup and an older sheaf's watermark settings filtered out of it for a
//! clean copy, and Export Flattened and print draw marks into their pages
//! with [`flatten_annots`], a post-pass on the checked graph.
//!
//! A file written to a path replaces it durably ([`durable::write`]). An
//! export never replaces the document's file ([`Snapshot::file`]).

use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use im::Vector;
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId, StringFormat, dictionary};

use crate::annot::{AnnotId, AnnotKind};
use crate::content::{PagOrigin, PaginationKind};
use crate::forms::FieldId;
use crate::materialize::{self, Encryption, Numbering, PostPass, Profile, SaveInput, SaveTarget};
use crate::model::{
    AnnotBody, Bookmark, DocState, FieldNode, Filter, FormDoc, LegacySel, Mark, MarkedSel,
    OutlineId, PageDef, PageId, Snapshot, Target, Terminal,
};
use crate::serialize::durable;

/// Serialize a snapshot as a complete new PDF: what a save writes, the
/// primary's encryption and object numbers kept.
pub fn rewrite(model: &Snapshot) -> Result<Vec<u8>> {
    written(model, model.state.clone(), None, Profile::default(), None)
}

/// How exports are written: encrypted as the primary is (M10.5 proposal
/// 10), numbered from 1 (§11.6).
const EXPORT: Profile = Profile {
    encryption: Encryption::Keep,
    numbering: Numbering::Dense,
};

/// `state`, `model`'s or what an export keeps of it, written by the new
/// writer as a Rewrite (to `path`, when it's for a file), as `profile`
/// says, with save policy's form transforms, which every way out applies
/// (`forms::rules::plan`), and `post` run on its checked graph. A save
/// asks what it asks before it writes, and exports ask nothing until slice
/// 19's preflight: the plan's asks are the callers'. What the writer
/// says of the file is logged.
fn written(
    model: &Snapshot,
    state: DocState,
    path: Option<&Path>,
    profile: Profile,
    post: Option<PostPass>,
) -> Result<Vec<u8>> {
    let input = SaveInput {
        target: state,
        ..SaveInput::detached(model)
    };
    let target = SaveTarget {
        // Bytes only: no file.
        path: path.unwrap_or(Path::new("")).into(),
        same_file: path.is_some_and(|p| is_the_file(model, p)),
        profile,
        // What's written here asks nothing: a save checks its file's
        // stamp (`materialize::save`).
        stamp: None,
    };
    let plan = materialize::plan(&input, &target);
    let out = materialize::materialize_then(&input, &plan, target, post, &in_progress::NoProgress)?;
    for n in &out.notes {
        log::warn!("{n}");
    }
    Ok(out.bytes)
}

/// PDFlyer's "Export Clean Copy": the document as it stands, with every
/// markup annotation removed, ours and anyone else's, and the settings an
/// older sheaf kept in its watermarks. Links, form fields and bookmarks
/// stay. Refuses to overwrite the open file.
pub fn export_clean_copy(model: &Snapshot, path: &Path) -> Result<()> {
    if is_the_file(model, path) {
        bail!("a clean copy can't replace the open file; choose another name");
    }
    let state = without_markup(&model.state);
    let bytes = written(model, state, Some(path), EXPORT, Some(&mut strip_markup))?;
    Ok(durable::write(path, &bytes, None)?)
}

/// `st` with no markup: sheaf's marks and other tools' comments go, and
/// what's left on each page is its links and widgets. The annotations the
/// records don't hold go as it's written ([`strip_markup`]). A page whose
/// content holds a watermark or background an older sheaf drew loses the
/// settings it kept there, an image's path among them, as it's written
/// ([`Filter::StripLegacy`]); renaming the resources it drew with is slice
/// 19's RemovePrivateData. Its pages' content is read to tell, each once a
/// source.
fn without_markup(st: &DocState) -> DocState {
    let annots = (st.pages()).map(|(id, _, _)| {
        let kept = (st.annots_on(id))
            .filter(|(_, rec)| match &rec.body {
                AnnotBody::Mark(_) => false,
                AnnotBody::Foreign(f) => f.summary.is_link(),
                AnnotBody::Widget(_) => true,
            })
            .map(|(aid, rec)| (aid, rec.body.clone()));
        (id, kept.collect())
    });
    let kinds = BTreeSet::from([PaginationKind::Watermark, PaginationKind::Background]);
    let origins = BTreeSet::from([PagOrigin::SheafLegacy]);
    let pages = (st.pages()).map(|(id, def, geom)| {
        let blocks = def.base.src.blocks(def.base.index);
        let def = match blocks.is_ok_and(|b| b.selected(&kinds, &origins).next().is_some()) {
            false => def.clone(),
            true => {
                let older = MarkedSel::Pagination {
                    kinds: kinds.clone(),
                    origins: origins.clone(),
                };
                let strip = Filter::StripLegacy(LegacySel::Marked(older));
                let content = def.content.with_filter(strip);
                Arc::new(PageDef::new(def.base.clone(), def.copy, content))
            }
        };
        (id, def, geom.clone())
    });
    let bookmarks = (st.bookmarks_preorder().into_iter())
        .filter_map(|(_, id, _)| Some((id, st.bookmark(id)?.clone())))
        .collect();
    DocState::builder()
        .pages(pages)
        .outline(st.outline_roots().clone(), bookmarks)
        .annots(annots)
        .labels(st.labels().cloned())
        .generators(st.generators().values().cloned())
        .opened_as(st)
        .form(
            (st.fields()).map(|(id, node)| (id, node.clone())),
            st.form().clone(),
        )
        .build()
}

/// The clean copy's post-pass: every annotation of every page that isn't
/// a link or a widget goes, those the records don't hold too (a screen, a
/// sound, a watermark annotation), as the old writer's clean copy dropped
/// them.
fn strip_markup(pdf: &mut Pdf) -> Result<()> {
    for page in pdf.get_pages().into_values() {
        let mut dict = pdf.get_dictionary(page)?.clone();
        strip_markup_annots(pdf, &mut dict);
        pdf.objects.insert(page, Object::Dictionary(dict));
    }
    Ok(())
}

/// Acrobat's Extract Pages: `ids` (kept in document order) become a new
/// file at `path`, along with their markup and the bookmarks that point at
/// them. Returns how many pages were written.
pub fn extract_pages(model: &Snapshot, ids: &[PageId], path: &Path) -> Result<usize> {
    let state = subset(&model.state, ids, true).context("no pages to extract")?;
    let bytes = written(model, state, Some(path), EXPORT, None)?;
    durable::write(path, &bytes, None)?;
    Ok(ids.len())
}

/// What [`extract_pages`] would write, in memory.
pub fn extract_bytes(model: &Snapshot, ids: &[PageId]) -> Result<Vec<u8>> {
    let state = subset(&model.state, ids, true).context("no pages to extract")?;
    written(model, state, None, EXPORT, None)
}

/// `ids` of `st` (in document order) with their markup and text layers,
/// and their bookmarks and page labels when `outline`. None when no page
/// is kept. What's left holds the state's invariants, as the cascades of
/// deleting the other pages would leave it (M10.5 §6.5): the form's nodes
/// keep only the widgets on kept pages, and a node that showed only on
/// pages left out goes, as `keep_only` takes such fields out of a file
/// (bug 14), with the parents left with no kids; a connector whose other
/// end is left out has no partner; a label range starts at the first page
/// of its range kept; and a generator is on the pages kept of its own,
/// with their Bates numbers.
pub(crate) fn subset(st: &DocState, ids: &[PageId], outline: bool) -> Option<DocState> {
    let keep: HashSet<PageId> = ids.iter().copied().collect();
    let pages: Vec<_> = (st.pages())
        .filter(|(id, _, _)| keep.contains(id))
        .map(|(id, def, geom)| (id, def.clone(), geom.clone()))
        .collect();
    if pages.is_empty() {
        return None;
    }
    let shown: HashSet<AnnotId> = (pages.iter())
        .flat_map(|(id, _, _)| st.annots_on(*id).map(|(a, _)| a))
        .collect();
    let annots: Vec<(PageId, Vec<_>)> = (pages.iter())
        .map(|(id, _, _)| {
            let list = st.annots_on(*id);
            let list = list.map(|(a, rec)| (a, paired(&rec.body, &shown)));
            (*id, list.collect())
        })
        .collect();
    let (roots, bookmarks) = if outline {
        prune_outline(st, &keep)
    } else {
        Default::default()
    };
    let (fields, form) = form_on(st, &shown);
    let labels = st.labels().filter(|_| outline).and_then(|specs| {
        let pos = |p| st.pos_of(p);
        match crate::labels::anchored(specs, st.order(), pos, |p| keep.contains(&p)) {
            None => Some(specs.clone()),
            Some((specs, _)) => (!specs.is_empty()).then(|| specs.into()),
        }
    });
    let generators =
        (st.generators().values()).filter_map(|g| g.on_pages_left(|p| keep.contains(&p)).0);
    let state = DocState::builder()
        .pages(pages)
        .outline(roots, bookmarks)
        .annots(annots)
        .labels(labels)
        .generators(generators)
        .opened_as(st)
        .form(fields, form)
        .build();
    debug_assert_eq!(state.check_invariants(), Ok(()));
    Some(state)
}

/// `body`, an annotation kept, with no partner when it's a connector whose
/// other end isn't among those `kept`.
fn paired(body: &AnnotBody, kept: &HashSet<AnnotId>) -> AnnotBody {
    match body {
        AnnotBody::Mark(m) if m.annot.partner().is_some_and(|p| !kept.contains(&p)) => {
            let mut mark = Mark::clone(m);
            if let AnnotKind::Connector { partner, .. } = &mut mark.annot.kind {
                *partner = None;
            }
            AnnotBody::Mark(Arc::new(mark))
        }
        body => body.clone(),
    }
}

/// What stays of `st`'s form when only the annotations `shown` are kept:
/// each terminal with the widgets among them, a node that showed and shows
/// nowhere now gone, and a parent whose kids all went gone with them. A
/// node that never had widgets or kids stays, as does a terminal with
/// widgets on page templates (`outside`). Its nodes, and the form with its
/// roots and calculation order the nodes left.
fn form_on(
    st: &DocState,
    shown: &HashSet<AnnotId>,
) -> (Vec<(FieldId, Arc<FieldNode>)>, Arc<FormDoc>) {
    fn keep(
        st: &DocState,
        id: FieldId,
        shown: &HashSet<AnnotId>,
        out: &mut Vec<(FieldId, Arc<FieldNode>)>,
    ) -> bool {
        let Some(node) = st.field(id) else {
            return false;
        };
        let kids: Vector<FieldId> = (node.kids.iter().copied())
            .filter(|&kid| keep(st, kid, shown, out))
            .collect();
        let term = node.term.as_ref().map(|t| {
            let widgets: Vector<AnnotId> = (t.widgets.iter().copied())
                .filter(|w| shown.contains(w))
                .collect();
            (t, widgets)
        });
        let had =
            !node.kids.is_empty() || term.as_ref().is_some_and(|(t, _)| !t.widgets.is_empty());
        let shows = (term.as_ref()).is_some_and(|(t, left)| t.outside > 0 || !left.is_empty());
        if had && !shows && kids.is_empty() {
            return false;
        }
        let unchanged = kids.len() == node.kids.len()
            && term
                .as_ref()
                .is_none_or(|(t, left)| left.len() == t.widgets.len());
        let node = if unchanged {
            node.clone()
        } else {
            let term = term.map(|(t, widgets)| {
                Arc::new(Terminal {
                    widgets,
                    ..Terminal::clone(t)
                })
            });
            Arc::new(FieldNode {
                kids,
                term,
                ..FieldNode::clone(node)
            })
        };
        out.push((id, node));
        true
    }
    let form = st.form();
    let mut fields = Vec::new();
    let roots: Vector<FieldId> = (form.roots.iter().copied())
        .filter(|&id| keep(st, id, shown, &mut fields))
        .collect();
    let left: HashSet<FieldId> = fields.iter().map(|(id, _)| *id).collect();
    let form = if left.len() == st.field_count() {
        form.clone()
    } else {
        let calc_order = (form.calc_order.iter().copied())
            .filter(|id| left.contains(id))
            .collect();
        Arc::new(FormDoc {
            roots,
            calc_order,
            ..FormDoc::clone(form)
        })
    };
    (fields, form)
}

/// The file sent to a printer: `ids` in document order, as they look
/// now. With `markup`, every comment and mark is drawn into the page (as
/// Export Flattened Copy does), so any printing system shows it; without,
/// markup is left out (Export Clean Copy). Form fields print either way.
/// Printed unencrypted: the bytes go to a printer or a page image, not to
/// a file someone keeps (M10.5 §12.3).
pub fn print_pdf(model: &Snapshot, ids: &[PageId], markup: bool) -> Result<Vec<u8>> {
    let state = subset(&model.state, ids, false).context("no pages to print")?;
    let profile = Profile {
        encryption: Encryption::None,
        numbering: Numbering::Dense,
    };
    if markup {
        let mut flatten = |pdf: &mut Pdf| flatten_annots(pdf).map(drop);
        written(model, state, None, profile, Some(&mut flatten))
    } else {
        let state = without_markup(&state);
        written(model, state, None, profile, Some(&mut strip_markup))
    }
}

/// Bookmarks that still point somewhere: one survives if its page is
/// kept or one under it survives (then it becomes a plain heading). One
/// going elsewhere, kept as read ([`Target::Kept`]), points at no page.
/// The top-level ones, and every one.
fn prune_outline(
    st: &DocState,
    keep: &HashSet<PageId>,
) -> (Vector<OutlineId>, im::HashMap<OutlineId, Arc<Bookmark>>) {
    fn level(
        st: &DocState,
        kids: &Vector<OutlineId>,
        keep: &HashSet<PageId>,
        out: &mut im::HashMap<OutlineId, Arc<Bookmark>>,
    ) -> Vector<OutlineId> {
        let mut kept = Vector::new();
        for &id in kids {
            let Some(b) = st.bookmark(id) else {
                continue;
            };
            let kids = level(st, &b.kids, keep, out);
            let target = b
                .target
                .clone()
                .filter(|t| matches!(t, Target::Page { page, .. } if keep.contains(page)));
            if target.is_none() && kids.is_empty() {
                continue;
            }
            let b = Bookmark {
                target,
                kids,
                ..Bookmark::clone(b)
            };
            out.insert(id, Arc::new(b));
            kept.push_back(id);
        }
        kept
    }
    let mut bookmarks = im::HashMap::new();
    let roots = level(st, st.outline_roots(), keep, &mut bookmarks);
    (roots, bookmarks)
}

/// Export with every markup annotation's appearance drawn into its page's
/// content and the annotations themselves removed, so the marks show in
/// any viewer and can't be edited. Links and form fields stay, as in the
/// clean copy; popups and hidden annotations are dropped without drawing.
pub fn export_flattened(model: &Snapshot, path: &Path) -> Result<usize> {
    if is_the_file(model, path) {
        bail!("a flattened copy can't replace the open file; choose another name");
    }
    let mut n = 0;
    let mut flatten = |pdf: &mut Pdf| {
        n = flatten_annots(pdf)?;
        Ok(())
    };
    let state = model.state.clone();
    let bytes = written(model, state, Some(path), EXPORT, Some(&mut flatten))?;
    durable::write(path, &bytes, None)?;
    Ok(n)
}

/// Draw annotation appearances into page content, as a post-pass on a
/// Rewrite's graph (`materialize::PostPass`). Returns how many were
/// flattened.
fn flatten_annots(pdf: &mut Pdf) -> Result<usize> {
    const HIDDEN: i64 = 1 << 1;
    const NOVIEW: i64 = 1 << 5;
    let mut flattened = 0usize;
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    for page_id in pages {
        let page = pdf.get_dictionary(page_id)?.clone();
        let Some(annots) = page
            .get(b"Annots")
            .ok()
            .and_then(|a| pdf.dereference(a).ok())
            .and_then(|(_, o)| o.as_array().ok().cloned())
        else {
            continue;
        };
        let mut kept: Vec<Object> = Vec::new();
        let mut ops = String::new();
        let mut xobjects: Vec<(String, ObjectId)> = Vec::new();
        for entry in &annots {
            let Ok((_, obj)) = pdf.dereference(entry) else {
                continue;
            };
            let Ok(dict) = obj.as_dict() else { continue };
            let subtype = dict
                .get(b"Subtype")
                .ok()
                .and_then(|s| s.as_name().ok())
                .unwrap_or(b"");
            if (subtype == b"Link" || subtype == b"Widget") && !dict.has(b"SheafAnnot") {
                kept.push(entry.clone());
                continue;
            }
            if subtype == b"Popup" {
                continue;
            }
            let flags = dict
                .get(b"F")
                .ok()
                .and_then(|f| f.as_i64().ok())
                .unwrap_or(0);
            if flags & (HIDDEN | NOVIEW) != 0 {
                continue;
            }
            let Some(stream_id) = normal_appearance(pdf, dict) else {
                continue;
            };
            let Ok(stream) = pdf.get_object(stream_id).and_then(|o| o.as_stream()) else {
                continue;
            };
            let rect: Vec<f32> = dict
                .get(b"Rect")
                .ok()
                .and_then(|o| pdf.dereference(o).ok())
                .and_then(|(_, o)| o.as_array().ok())
                .map(|a| {
                    a.iter()
                        .map(|v| v.as_float().unwrap_or(0.0) as f32)
                        .collect()
                })
                .unwrap_or_default();
            if rect.len() != 4 {
                continue;
            }
            let (llx, lly, urx, ury) = (
                rect[0].min(rect[2]),
                rect[1].min(rect[3]),
                rect[0].max(rect[2]),
                rect[1].max(rect[3]),
            );
            // PDF 32000 12.5.5: map the form's (Matrix-transformed) BBox onto Rect.
            let bbox: Vec<f32> = stream
                .dict
                .get(b"BBox")
                .ok()
                .and_then(|o| o.as_array().ok())
                .map(|a| {
                    a.iter()
                        .map(|v| v.as_float().unwrap_or(0.0) as f32)
                        .collect()
                })
                .unwrap_or_else(|| vec![llx, lly, urx, ury]);
            let m: Vec<f32> = stream
                .dict
                .get(b"Matrix")
                .ok()
                .and_then(|o| o.as_array().ok())
                .map(|a| {
                    a.iter()
                        .map(|v| v.as_float().unwrap_or(0.0) as f32)
                        .collect()
                })
                .filter(|m: &Vec<f32>| m.len() == 6)
                .unwrap_or_else(|| vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
            if bbox.len() != 4 {
                continue;
            }
            let xf = |x: f32, y: f32| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
            let corners = [
                xf(bbox[0], bbox[1]),
                xf(bbox[2], bbox[1]),
                xf(bbox[2], bbox[3]),
                xf(bbox[0], bbox[3]),
            ];
            let bx0 = corners.iter().map(|c| c.0).fold(f32::MAX, f32::min);
            let bx1 = corners.iter().map(|c| c.0).fold(f32::MIN, f32::max);
            let by0 = corners.iter().map(|c| c.1).fold(f32::MAX, f32::min);
            let by1 = corners.iter().map(|c| c.1).fold(f32::MIN, f32::max);
            let sx = if bx1 - bx0 > 1e-6 {
                (urx - llx) / (bx1 - bx0)
            } else {
                1.0
            };
            let sy = if by1 - by0 > 1e-6 {
                (ury - lly) / (by1 - by0)
            } else {
                1.0
            };
            let (tx, ty) = (llx - bx0 * sx, lly - by0 * sy);
            let name = format!("SheafFlat{}", xobjects.len() + 1);
            ops.push_str(&format!(
                " q {sx:.4} 0 0 {sy:.4} {tx:.3} {ty:.3} cm /{name} Do Q"
            ));
            xobjects.push((name, stream_id));
            flattened += 1;
        }
        if xobjects.is_empty() && kept.len() == annots.len() {
            continue;
        }
        // Register the forms as page XObjects and append the draws after
        // the existing content, which is wrapped so its state can't leak.
        let mut resources = page
            .get(b"Resources")
            .ok()
            .and_then(|r| pdf.dereference(r).ok())
            .and_then(|(_, o)| o.as_dict().ok().cloned())
            .unwrap_or_default();
        let mut xo = resources
            .get(b"XObject")
            .ok()
            .and_then(|r| pdf.dereference(r).ok())
            .and_then(|(_, o)| o.as_dict().ok().cloned())
            .unwrap_or_default();
        for (name, id) in &xobjects {
            xo.set(name.clone(), Object::Reference(*id));
        }
        resources.set("XObject", Object::Dictionary(xo));
        let mut content = b"q\n".to_vec();
        content.extend_from_slice(&pdf.get_page_content(page_id));
        content.extend_from_slice(b"\nQ\n");
        content.extend_from_slice(ops.as_bytes());
        let mut content = lopdf::Stream::new(dictionary! {}, content);
        let _ = content.compress();
        let content_id = pdf.add_object(Object::Stream(content));
        let page_mut = pdf.get_dictionary_mut(page_id)?;
        page_mut.set("Contents", Object::Reference(content_id));
        page_mut.set("Resources", Object::Dictionary(resources));
        if kept.is_empty() {
            page_mut.remove(b"Annots");
        } else {
            page_mut.set("Annots", Object::Array(kept));
        }
    }
    Ok(flattened)
}

/// The annotation's normal appearance stream, resolving appearance
/// sub-dictionaries through `/AS` (or a lone state).
fn normal_appearance(pdf: &Pdf, annot: &Dictionary) -> Option<ObjectId> {
    let ap = annot
        .get(b"AP")
        .ok()
        .and_then(|o| pdf.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok().cloned())?;
    let n = ap.get(b"N").ok()?;
    if let Ok(id) = n.as_reference()
        && pdf.get_object(id).ok()?.as_stream().is_ok()
    {
        return Some(id);
    }
    let (_, states) = pdf.dereference(n).ok()?;
    let states = states.as_dict().ok()?;
    let state = annot
        .get(b"AS")
        .ok()
        .and_then(|s| s.as_name().ok())
        .map(|s| s.to_vec())
        .or_else(|| states.iter().next().map(|(k, _)| k.clone()))?;
    states.get(&state).ok()?.as_reference().ok()
}

/// Whether `path` names `model`'s document's file ([`Snapshot::file`]),
/// which an export never replaces.
fn is_the_file(model: &Snapshot, path: &Path) -> bool {
    (model.file.as_deref()).is_some_and(|file| durable::same_file(file, path))
}

/// Rewrite to `path` durably (a temp file renamed over it), as a save
/// would ([`rewrite`]).
pub fn rewrite_to_path(model: &Snapshot, path: &Path) -> Result<()> {
    let state = model.state.clone();
    let bytes = written(model, state, Some(path), Profile::default(), None)?;
    Ok(durable::write(path, &bytes, None)?)
}

/// PDF text string: as is when PDFDocEncoding reads it back so
/// (`file::is_pdf_doc_ascii`), otherwise UTF-16BE with BOM.
pub fn text_string(s: &str) -> Object {
    if crate::file::is_pdf_doc_ascii(s) {
        Object::String(s.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        Object::String(bytes, StringFormat::Hexadecimal)
    }
}

/// Sanity check used by tests and the app: every page shows a page that
/// exists in its source.
pub fn validate(model: &Snapshot) -> Result<()> {
    for (id, def, _) in model.state.pages() {
        if def.base.index >= def.base.src.page_count() {
            bail!(
                "page {id:?} shows page {} of its source, which hasn't got it",
                def.base.index,
            );
        }
    }
    Ok(())
}

/// Is this /Annots entry navigation or a form field rather than markup?
/// Our connectors are links too, but they're markup.
fn is_link_or_field(pdf: &Pdf, entry: &Object) -> bool {
    pdf.dereference(entry)
        .ok()
        .and_then(|(_, o)| o.as_dict().ok())
        .filter(|d| !d.has(b"SheafAnnot"))
        .and_then(|d| d.get(b"Subtype").ok())
        .and_then(|s| s.as_name().ok())
        .is_some_and(|n| n == b"Link" || n == b"Widget")
}

/// Drop every markup annotation from a page, keeping links and form fields.
pub(crate) fn strip_markup_annots(pdf: &Pdf, page: &mut Dictionary) {
    let Some(existing) = page
        .get(b"Annots")
        .ok()
        .and_then(|a| pdf.dereference(a).ok())
        .and_then(|(_, o)| o.as_array().ok().cloned())
    else {
        return;
    };
    let kept: Vec<Object> = existing
        .into_iter()
        .filter(|e| is_link_or_field(pdf, e))
        .collect();
    if kept.is_empty() {
        page.remove(b"Annots");
    } else {
        page.set("Annots", Object::Array(kept));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annot::Annot;
    use crate::history::FixedCount;
    use crate::materialize::rewrite_tests::{
        HELLO_HEX, annots_read, fonts_of, make_pdf, open, read_outline, read_pages, set_layer,
        tempdir,
    };
    use crate::model::FlatOutlineItem;
    use crate::tx::fixture as fx;
    use sheaf_edit::{Rotation, Turn};

    #[test]
    fn rewrite_to_path_is_atomic() {
        let dir = tempdir();
        let a = make_pdf(&dir, "a.pdf", "A", 2, &[]);
        let model = open(&a, &[], Box::new(FixedCount(10)));
        let out = dir.join("out.pdf");
        rewrite_to_path(&model.snapshot(), &out).unwrap();
        assert!(out.exists());
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(
            !names
                .iter()
                .any(|n| n.to_string_lossy().ends_with(".sheaf-tmp")),
            "{names:?}"
        );
        assert_eq!(read_pages(&std::fs::read(&out).unwrap()).len(), 2);
    }

    /// A clean copy keeps pages, bookmarks, links and form fields, and drops
    /// every markup annotation — ours and third-party — along with their
    /// now-unreferenced objects.
    #[test]
    fn clean_copy_strips_markup_only() {
        use crate::annot::{AnnotKind, Box2};
        let dir = tempdir();
        let a_path = make_pdf(&dir, "m.pdf", "M", 2, &[("Top", 0, 0), ("Second", 1, 0)]);
        // Plant third-party annotations on page 1: a note with its popup, a
        // link and a form field.
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
        let mut model = open(
            &a_path,
            &[
                FlatOutlineItem {
                    title: "Top".into(),
                    page: Some(0),
                    depth: 0,
                    view: None,
                    color: None,
                    style: Default::default(),
                    open: true,
                    object: None,
                    kept: false,
                },
                FlatOutlineItem {
                    title: "Second".into(),
                    page: Some(1),
                    depth: 0,
                    view: None,
                    color: None,
                    style: Default::default(),
                    open: true,
                    object: None,
                    kept: false,
                },
            ],
            Box::new(FixedCount(10)),
        );
        let p0 = model.page(0).unwrap();
        model
            .commit_ops(
                vec![fx::add(
                    p0,
                    &Annot {
                        id: crate::annot::AnnotId(0),
                        kind: AnnotKind::Stamp {
                            rect: Box2 {
                                x: 400.0,
                                y: 80.0,
                                w: 24.0,
                                h: 24.0,
                            },
                            glyph: "✓".into(),
                        },
                        color: [0, 150, 0],
                        opacity: 1.0,
                        stroke: 1.0,
                        author: "greg".into(),
                        contents: String::new(),
                        modified: "D:20261011000000".into(),
                        created: String::new(),
                        fill: None,
                        dashed: false,
                        cloudy: false,
                        turn: Rotation::R0,
                        name: String::new(),
                    },
                )],
                "tick",
            )
            .unwrap();
        // A connector pair: links in the file, but ours, so they go too.
        let p1 = model.page(1).unwrap();
        let conn = || Annot {
            id: crate::annot::AnnotId(0),
            kind: AnnotKind::Connector {
                rect: Box2 {
                    x: 50.0,
                    y: 50.0,
                    w: 22.0,
                    h: 22.0,
                },
                label: "1".into(),
                partner: None,
            },
            color: [220, 40, 40],
            opacity: 1.0,
            stroke: 1.0,
            author: "greg".into(),
            contents: String::new(),
            modified: "D:20261011000000".into(),
            created: String::new(),
            fill: None,
            dashed: false,
            cloudy: false,
            turn: Rotation::R0,
            name: String::new(),
        };
        model
            .commit_ops(vec![fx::connect((p0, &conn()), (p1, &conn()))], "conn")
            .unwrap();
        // Also rotate page 2 so the copy reflects edits, not just the file.
        model
            .commit_ops(vec![fx::rotate(&[p1], Turn::Quarter)], "rot")
            .unwrap();

        let out = dir.join("clean.pdf");
        export_clean_copy(&model.snapshot(), &out).unwrap();
        let pdf = Pdf::load(&out).unwrap();
        let pages = pdf.get_pages();
        assert_eq!(pages.len(), 2);
        let annots = pdf
            .dereference(
                pdf.get_dictionary(pages[&1])
                    .unwrap()
                    .get(b"Annots")
                    .unwrap(),
            )
            .unwrap()
            .1
            .as_array()
            .unwrap()
            .clone();
        let mut subtypes: Vec<String> = annots
            .iter()
            .map(|a| {
                String::from_utf8_lossy(
                    pdf.get_dictionary(a.as_reference().unwrap())
                        .unwrap()
                        .get(b"Subtype")
                        .unwrap()
                        .as_name()
                        .unwrap(),
                )
                .into_owned()
            })
            .collect();
        subtypes.sort();
        assert_eq!(subtypes, ["Link", "Widget"]);
        assert!(!pdf.get_dictionary(pages[&2]).unwrap().has(b"Annots"));
        assert_eq!(
            pdf.get_dictionary(pages[&2])
                .unwrap()
                .get(b"Rotate")
                .unwrap()
                .as_i64()
                .unwrap(),
            90
        );
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
                assert!(!d.has(b"SheafAnnot"));
            }
        }
        assert!(
            annots_read(&std::fs::read(&out).unwrap())
                .sheaf()
                .is_empty()
        );
        // Bookmarks kept.
        assert!(
            pdf.get_dictionary(pdf.trailer.get(b"Root").unwrap().as_reference().unwrap())
                .unwrap()
                .has(b"Outlines")
        );
        // The open file itself can't be the target.
        assert!(export_clean_copy(&model.snapshot(), &a_path).is_err());
        assert_eq!(std::fs::read(&a_path).unwrap(), original);
    }

    /// Extract keeps the chosen pages in order, their marks, and the
    /// bookmarks that point at them (parents of kept children become
    /// headings).
    #[test]
    fn extract_pages_keeps_marks_and_relevant_bookmarks() {
        use crate::annot::{AnnotKind, Box2};
        let dir = tempdir();
        let a = make_pdf(&dir, "x.pdf", "X", 4, &[]);
        let mut model = open(
            &a,
            &[
                FlatOutlineItem {
                    title: "Part".into(),
                    page: Some(0),
                    depth: 0,
                    view: None,
                    color: None,
                    style: Default::default(),
                    open: true,
                    object: None,
                    kept: false,
                },
                FlatOutlineItem {
                    title: "Second".into(),
                    page: Some(1),
                    depth: 1,
                    view: None,
                    color: None,
                    style: Default::default(),
                    open: true,
                    object: None,
                    kept: false,
                },
                FlatOutlineItem {
                    title: "Fourth".into(),
                    page: Some(3),
                    depth: 1,
                    view: None,
                    color: None,
                    style: Default::default(),
                    open: true,
                    object: None,
                    kept: false,
                },
                FlatOutlineItem {
                    title: "Third".into(),
                    page: Some(2),
                    depth: 0,
                    view: None,
                    color: None,
                    style: Default::default(),
                    open: true,
                    object: None,
                    kept: false,
                },
            ],
            Box::new(FixedCount(10)),
        );
        let ids: Vec<PageId> = (0..4).map(|i| model.page(i).unwrap()).collect();
        for (i, kind) in [
            (
                1usize,
                AnnotKind::Square {
                    rect: Box2 {
                        x: 10.0,
                        y: 10.0,
                        w: 50.0,
                        h: 20.0,
                    },
                },
            ),
            (
                2usize,
                AnnotKind::Note {
                    pos: crate::annot::Pt { x: 5.0, y: 5.0 },
                },
            ),
        ] {
            model
                .commit_ops(
                    vec![fx::add(
                        ids[i],
                        &Annot {
                            id: crate::annot::AnnotId(0),
                            kind,
                            color: [0, 0, 0],
                            opacity: 1.0,
                            stroke: 1.0,
                            author: "t".into(),
                            contents: String::new(),
                            modified: String::new(),
                            created: String::new(),
                            fill: None,
                            dashed: false,
                            cloudy: false,
                            turn: Rotation::R0,
                            name: String::new(),
                        },
                    )],
                    "a",
                )
                .unwrap();
        }
        let out = dir.join("extract.pdf");
        // Pages 2 and 4 (ids given out of order: output keeps document order).
        let n = extract_pages(&model.snapshot(), &[ids[3], ids[1]], &out).unwrap();
        assert_eq!(n, 2);
        let pdf = Pdf::load(&out).unwrap();
        assert_eq!(pdf.get_pages().len(), 2);
        let text = |i: u32| {
            String::from_utf8_lossy(&pdf.get_page_content(pdf.get_pages()[&i])).into_owned()
        };
        assert!(text(1).contains("MARK X 1"), "{}", text(1));
        assert!(text(2).contains("MARK X 3"), "{}", text(2));
        // Only page 2's square came along.
        let marks = annots_read(&std::fs::read(&out).unwrap()).sheaf();
        assert_eq!(marks.len(), 1);
        assert!(matches!(marks[&0][0].kind, AnnotKind::Square { .. }));
        // Bookmarks: Part (now a heading) > Second, Fourth; Third dropped.
        let items = read_outline(&std::fs::read(&out).unwrap());
        let titles: Vec<(u16, String, Option<u32>)> = items;
        assert_eq!(
            titles,
            vec![
                (0, "Part".into(), None),
                (1, "Second".into(), Some(1)),
                (1, "Fourth".into(), Some(2)),
            ]
        );
        assert!(extract_pages(&model.snapshot(), &[], &out).is_err());
    }

    #[test]
    fn print_pdf_flattens_or_drops_markup() {
        use crate::annot::{AnnotKind, Box2};
        let dir = tempdir();
        let a = make_pdf(&dir, "p.pdf", "P", 3, &[]);
        let mut model = open(
            &a,
            &[FlatOutlineItem {
                title: "Top".into(),
                page: Some(1),
                depth: 0,
                view: None,
                color: None,
                style: Default::default(),
                open: true,
                object: None,
                kept: false,
            }],
            Box::new(FixedCount(10)),
        );
        let ids: Vec<PageId> = (0..3).map(|i| model.page(i).unwrap()).collect();
        model
            .commit_ops(
                vec![fx::add(
                    ids[1],
                    &Annot {
                        id: crate::annot::AnnotId(0),
                        kind: AnnotKind::Square {
                            rect: Box2 {
                                x: 10.0,
                                y: 10.0,
                                w: 50.0,
                                h: 20.0,
                            },
                        },
                        color: [255, 0, 0],
                        opacity: 1.0,
                        stroke: 1.0,
                        author: "t".into(),
                        contents: String::new(),
                        modified: String::new(),
                        created: String::new(),
                        fill: None,
                        dashed: false,
                        cloudy: false,
                        turn: Rotation::R0,
                        name: String::new(),
                    },
                )],
                "a",
            )
            .unwrap();
        let snap = model.snapshot();
        let with = Pdf::load_mem(&print_pdf(&snap, &[ids[2], ids[1]], true).unwrap()).unwrap();
        assert_eq!(with.get_pages().len(), 2);
        let first = with.get_pages()[&1];
        let content = String::from_utf8_lossy(&with.get_page_content(first)).into_owned();
        assert!(content.contains("MARK P 1"), "{content}");
        // The square is drawn into the page, not left as an annotation.
        let annots = with.get_page_annotations(first).unwrap_or_default();
        assert!(annots.is_empty());
        assert!(content.len() > "MARK P 1".len() + 20, "{content}");
        assert!(with.catalog().unwrap().get(b"Outlines").is_err());
        let without = Pdf::load_mem(&print_pdf(&snap, &[ids[1]], false).unwrap()).unwrap();
        let only = without.get_pages()[&1];
        assert!(
            without
                .get_page_annotations(only)
                .unwrap_or_default()
                .is_empty()
        );
        let plain = String::from_utf8_lossy(&without.get_page_content(only)).into_owned();
        assert!(plain.len() < content.len(), "{plain}");
        assert!(print_pdf(&snap, &[], true).is_err());
    }

    /// Flattening draws each mark's appearance into the page content and
    /// removes the annotation; links stay.
    #[test]
    fn flattened_copy_burns_marks_into_content() {
        use crate::annot::{AnnotKind, Box2};
        let dir = tempdir();
        let a = make_pdf(&dir, "f.pdf", "F", 1, &[]);
        {
            let mut pdf = Pdf::load(&a).unwrap();
            let page0 = pdf.get_pages()[&1];
            let link = pdf.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Link",
                "Rect" => Object::Array(vec![0.into(), 0.into(), 50.into(), 20.into()]),
            });
            pdf.get_dictionary_mut(page0)
                .unwrap()
                .set("Annots", Object::Array(vec![link.into()]));
            pdf.save(&a).unwrap();
        }
        let mut model = open(&a, &[], Box::new(FixedCount(10)));
        let p0 = model.page(0).unwrap();
        model
            .commit_ops(
                vec![fx::add(
                    p0,
                    &Annot {
                        id: crate::annot::AnnotId(0),
                        kind: AnnotKind::Square {
                            rect: Box2 {
                                x: 100.0,
                                y: 100.0,
                                w: 80.0,
                                h: 40.0,
                            },
                        },
                        color: [0, 120, 255],
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
                    },
                )],
                "sq",
            )
            .unwrap();
        let out = dir.join("flat.pdf");
        assert_eq!(export_flattened(&model.snapshot(), &out).unwrap(), 1);
        let pdf = Pdf::load(&out).unwrap();
        let page = pdf.get_pages()[&1];
        let dict = pdf.get_dictionary(page).unwrap();
        // Only the link remains as an annotation.
        let annots = pdf
            .dereference(dict.get(b"Annots").unwrap())
            .unwrap()
            .1
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(annots.len(), 1);
        assert!(
            annots_read(&std::fs::read(&out).unwrap())
                .sheaf()
                .is_empty()
        );
        // The content draws the form XObject, which is registered in Resources.
        let content = String::from_utf8_lossy(&pdf.get_page_content(page)).into_owned();
        assert!(content.contains("/SheafFlat1 Do"), "{content}");
        assert!(content.starts_with("q\n"), "{content}");
        let (res, _) = pdf.get_page_resources(page).unwrap();
        let xo = res.unwrap().get(b"XObject").unwrap().as_dict().unwrap();
        let form = pdf
            .get_object(xo.get(b"SheafFlat1").unwrap().as_reference().unwrap())
            .unwrap()
            .as_stream()
            .unwrap();
        assert_eq!(
            form.dict.get(b"Subtype").unwrap().as_name().unwrap(),
            b"Form"
        );
        // The form content is the square's stroke.
        let form_ops = String::from_utf8_lossy(&form.decompressed_content().unwrap()).into_owned();
        assert!(form_ops.contains(" re\nS\n"), "{form_ops}");
    }

    #[test]
    fn extract_flatten_and_clean_keep_layer() {
        use crate::annot::{AnnotKind, Box2};
        let dir = tempdir();
        let a = make_pdf(&dir, "a.pdf", "A", 2, &[]);
        let mut model = open(&a, &[], Box::new(FixedCount(10)));
        let ids: Vec<PageId> = model.state().order().iter().copied().collect();
        set_layer(&mut model, ids[1], "Hello");
        model
            .commit_ops(
                vec![fx::add(
                    ids[1],
                    &Annot {
                        id: crate::annot::AnnotId(0),
                        kind: AnnotKind::Square {
                            rect: Box2 {
                                x: 100.0,
                                y: 100.0,
                                w: 80.0,
                                h: 40.0,
                            },
                        },
                        color: [255, 0, 0],
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
                    },
                )],
                "sq",
            )
            .unwrap();
        let snap = model.snapshot();

        let out = dir.join("extract.pdf");
        assert_eq!(extract_pages(&snap, &[ids[1]], &out).unwrap(), 1);
        let pdf = Pdf::load(&out).unwrap();
        let page = pdf.get_pages()[&1];
        assert!(String::from_utf8_lossy(&pdf.get_page_content(page)).contains(HELLO_HEX));

        let out = dir.join("flat.pdf");
        assert_eq!(export_flattened(&snap, &out).unwrap(), 1);
        let pdf = Pdf::load(&out).unwrap();
        let page = pdf.get_pages()[&2];
        let all = String::from_utf8_lossy(&pdf.get_page_content(page)).into_owned();
        assert!(all.contains(HELLO_HEX), "{all}");
        assert!(all.contains("/SheafFlat1 Do"), "{all}");
        assert!(fonts_of(&pdf, page).has(b"SheafOcr"));

        let out = dir.join("clean.pdf");
        export_clean_copy(&snap, &out).unwrap();
        let pdf = Pdf::load(&out).unwrap();
        let page = pdf.get_pages()[&2];
        assert!(String::from_utf8_lossy(&pdf.get_page_content(page)).contains(HELLO_HEX));
        assert!(!pdf.get_dictionary(page).unwrap().has(b"Annots"));
    }
}
