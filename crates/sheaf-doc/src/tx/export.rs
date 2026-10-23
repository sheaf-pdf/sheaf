//! Export transactions (M10.5 §6.3, §12): an export's transforms lowered
//! as the binder lowers its ops, on the version it exports, and never
//! committed. Each transform is resolved, allowed and lowered in turn, its
//! cascades settled as an op's are; an `Op` goes through [`Tx::apply`]
//! itself. What an export's transforms make draws its ids from the
//! document's allocator, as an edit's would. Only [`ExportTx`] takes a
//! [`Transform`], so nothing an export does can become an edit.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use im::Vector;
use in_progress::Progress;
use sheaf_edit::{MarkupKeep, OutlinePolicy, PageNumbering, PageSel, Transform};

use super::caps::{actor_exports, transform_cap};
use super::cascade;
use super::content::private_filters;
use super::marks::drop_annots;
use super::resolve::{ResolvedOp, Resolver};
use super::{EditError, Lower, LowerCx, Tx};
use crate::materialize::{Private, Unheld};
use crate::model::{
    AnnotBody, Bookmark, DocState, Document, OutlineId, PageDef, PageId, Snapshot, Target,
};
use crate::prim::{Key, Rec, Val};

/// An export's transforms, lowered on the version it exports (§6.3): the
/// binder's resolve, capability and lowering stages, its cascades, its ids;
/// never committed. What it says beside its state is how the export is
/// written: without sheaf's private data once it's taken out, and with only
/// the links and widgets the records don't hold once markup is.
pub struct ExportTx {
    tx: Tx,
    private: Private,
    unheld: Unheld,
    flattened: usize,
}

impl ExportTx {
    /// An export of `snap`'s version, lowering with `cx`.
    pub fn new(snap: &Snapshot, cx: Arc<LowerCx>) -> ExportTx {
        ExportTx {
            tx: Document::tx_at(snap, cx),
            private: Private::Keep,
            unheld: Unheld::Keep,
            flattened: 0,
        }
    }

    /// `transform`, lowered: all of it, or nothing and why not. Canceled
    /// by `p`, it leaves the transaction as it was.
    pub fn apply(&mut self, transform: Transform, p: &dyn Progress) -> Result<(), EditError> {
        self.tx.ready()?;
        let name = transform_name(&transform);
        if let Some(cap) = transform_cap(&transform)
            && !actor_exports(self.tx.actor(), cap)
        {
            return Err(EditError::Refused(cap));
        }
        match transform {
            Transform::Op(op) => {
                self.tx.apply(op, p)?;
                let last = self.tx.steps.last().map(|s| &*s.op);
                if let Some(ResolvedOp::FlattenAnnotations { flattened, .. }) = last {
                    self.flattened += flattened;
                }
            }
            Transform::KeepPages {
                pages,
                outline,
                labels,
                numbering,
            } => {
                let keep = self.pages(name, &pages)?;
                // Where the pages' page tokens read each is: among those
                // kept, or where the version they're kept from has it.
                let from = match numbering {
                    PageNumbering::AsKept => None,
                    PageNumbering::AsDocument => Some(self.tx.state.clone()),
                };
                self.lower(name, p, |lw| keep_pages(lw, &keep, outline, labels))?;
                self.tx.state.number_in(from);
            }
            Transform::RemoveMarkup { keep } => {
                self.lower(name, p, |lw| remove_markup(lw, keep))?;
                // What an earlier removal took off stays off.
                let keep = match self.unheld {
                    Unheld::Keep => keep,
                    Unheld::Only(was) => MarkupKeep {
                        links: was.links && keep.links,
                        widgets: was.widgets && keep.widgets,
                    },
                };
                self.unheld = Unheld::Only(keep);
            }
            Transform::RemovePrivateData => {
                self.lower(name, p, remove_private_data)?;
                self.private = Private::Strip;
            }
            _ => {
                return Err(EditError::invalid(name, "no export lowers it"));
            }
        }
        Ok(())
    }

    /// The state its transforms made: what the export writes.
    pub fn state(&self) -> &DocState {
        self.tx.state()
    }

    /// Whether the export holds sheaf's private data: [`Private::Strip`]
    /// once it's taken out.
    pub fn private(&self) -> Private {
        self.private
    }

    /// Which annotations the records don't hold the export keeps: only the
    /// links and widgets a markup removal keeps, once there's been one.
    pub fn unheld(&self) -> Unheld {
        self.unheld
    }

    /// How many annotations its flattens drew into their pages.
    pub fn flattened(&self) -> usize {
        self.flattened
    }

    /// The pages `sel` names, in the state as it is now.
    fn pages(&self, name: &'static str, sel: &PageSel) -> Result<Vec<PageId>, EditError> {
        let tx = &self.tx;
        let mut r = Resolver {
            st: &tx.state,
            locals: &tx.locals,
            cx: &tx.cx,
            grants: &tx.grants,
            named: &tx.named,
            from_file: tx.from_file,
            op: name,
            build: false,
            reads: BTreeSet::new(),
        };
        r.pages(sel)
    }

    /// `lower` run on the state as it is now, as transform `name`, and its
    /// cascades settled: the state it leaves is the transaction's, or, when
    /// it fails, nothing changes.
    fn lower(
        &mut self,
        name: &'static str,
        p: &dyn Progress,
        lower: impl FnOnce(&mut Lower) -> Result<(), EditError>,
    ) -> Result<(), EditError> {
        let mut lw = Lower::new(Rec::new(self.tx.state.clone()), &self.tx.cx, p, name);
        lower(&mut lw)?;
        cascade::settle(&mut lw)?;
        let (state, _) = lw.rec.finish();
        debug_assert_eq!(state.check_invariants(), Ok(()), "{name}");
        self.tx.state = state;
        Ok(())
    }
}

/// What `transform` is called in its JSON, as its errors say: an op's
/// name for an op.
pub(crate) fn transform_name(transform: &Transform) -> &'static str {
    match transform {
        Transform::Op(op) => super::op_name(op),
        Transform::KeepPages { .. } => "keep-pages",
        Transform::RemoveMarkup { .. } => "remove-markup",
        Transform::RemovePrivateData => "remove-private-data",
        _ => "transform",
    }
}

/// `keep`, in document order, and what's on them: the other pages deleted,
/// with the cascades of a delete (their annotations, a connector's
/// partner, the fields shown only there and the parents they empty, a
/// label range starting there, a generator's pages). The outline as
/// `outline` says, and no page labels unless `labels`.
fn keep_pages(
    lw: &mut Lower,
    keep: &[PageId],
    outline: OutlinePolicy,
    labels: bool,
) -> Result<(), EditError> {
    if keep.is_empty() {
        return Err(lw.invalid("it keeps no page"));
    }
    let kept: HashSet<PageId> = keep.iter().copied().collect();
    match outline {
        OutlinePolicy::Headings => headings(&mut lw.rec, &kept),
        // The delete's cascade.
        OutlinePolicy::Promote => {}
        OutlinePolicy::Drop => no_outline(&mut lw.rec),
    }
    if !labels {
        lw.rec.put(Key::Labels, None);
    }
    let gone: Vec<PageId> = (lw.rec.order().iter())
        .filter(|id| !kept.contains(id))
        .copied()
        .collect();
    super::pages::delete(lw, &gone)
}

/// The outline of the pages `kept`: a bookmark stays while its page does;
/// one whose page goes, or that goes nowhere in the file (a link, a script,
/// [`Target::Kept`]), stays as a heading going nowhere when one under it
/// stays, and goes otherwise, with what's under it.
fn headings(rec: &mut Rec, kept: &HashSet<PageId>) {
    /// Of `kids`, those that stay, so treated; each one that changes or
    /// goes into `out`.
    fn level(
        st: &DocState,
        kids: &Vector<OutlineId>,
        kept: &HashSet<PageId>,
        out: &mut Vec<(OutlineId, Option<Arc<Bookmark>>)>,
    ) -> Vector<OutlineId> {
        let mut stay = Vector::new();
        for &id in kids {
            let Some(b) = st.bookmark(id) else {
                continue;
            };
            let below = level(st, &b.kids, kept, out);
            let target = (b.target.clone())
                .filter(|t| matches!(t, Target::Page { page, .. } if kept.contains(page)));
            if target.is_none() && below.is_empty() {
                out.push((id, None));
                continue;
            }
            if target != b.target || below != b.kids {
                let b = Bookmark {
                    target,
                    kids: below,
                    ..Bookmark::clone(b)
                };
                out.push((id, Some(Arc::new(b))));
            }
            stay.push_back(id);
        }
        stay
    }
    let mut out = Vec::new();
    let st: &DocState = rec;
    let roots = level(st, st.outline_roots(), kept, &mut out);
    if out.is_empty() {
        return;
    }
    for (id, b) in out {
        rec.put(Key::Bookmark(id), b.map(Val::Bookmark));
    }
    rec.put(Key::OutlineRoots, Some(Val::OutlineRoots(roots)));
}

/// No bookmarks.
fn no_outline(rec: &mut Rec) {
    let all: Vec<OutlineId> = (rec.bookmarks_preorder().into_iter())
        .map(|(_, id, _)| id)
        .collect();
    if all.is_empty() {
        return;
    }
    for id in all {
        rec.put(Key::Bookmark(id), None);
    }
    rec.put(Key::OutlineRoots, Some(Val::OutlineRoots(Vector::new())));
}

/// Every page's marks and other tools' comments taken off, each with its
/// thread, and its links and widgets as `keep` says (a widget from its
/// field, which goes with its last). What the records don't hold the
/// export leaves out as it writes ([`Unheld::Only`]).
fn remove_markup(lw: &mut Lower, keep: MarkupKeep) -> Result<(), EditError> {
    let pages: Vec<PageId> = lw.rec.order().iter().copied().collect();
    for (i, &page) in pages.iter().enumerate() {
        lw.pages_done(i, pages.len())?;
        drop_annots(&mut lw.rec, page, |_, a| match &a.body {
            AnnotBody::Mark(_) => true,
            AnnotBody::Foreign(f) if f.summary.is_link() => !keep.links,
            AnnotBody::Foreign(_) => true,
            AnnotBody::Widget(_) => !keep.widgets,
        });
    }
    Ok(())
}

/// Each page's content with the filters that take sheaf's private data out
/// of it ([`private_filters`]), for an export written without it
/// ([`Private::Strip`]).
fn remove_private_data(lw: &mut Lower) -> Result<(), EditError> {
    let pages: Vec<(PageId, Arc<PageDef>)> = (lw.rec.pages())
        .map(|(id, def, _)| (id, def.clone()))
        .collect();
    for (i, (id, def)) in pages.iter().enumerate() {
        lw.pages_done(i, pages.len())?;
        let filters = private_filters(def);
        if filters.is_empty() {
            continue;
        }
        let content = def.content.with_filters(filters);
        let def = PageDef::new(def.base.clone(), def.copy, content);
        lw.rec.put(Key::Page(*id), Some(Val::Page(Arc::new(def))));
    }
    Ok(())
}
