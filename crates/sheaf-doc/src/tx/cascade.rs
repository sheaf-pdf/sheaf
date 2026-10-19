//! Cascades, declared once (M10.5 §6.5): named rules that keep a state
//! consistent after an op. Each is pure and idempotent: it reads what the
//! last round wrote, writes ordinary prims into the same step, and finds
//! nothing more to do in its own output. They run in rounds until a round
//! writes nothing; more than [`MAX_ROUNDS`] is a bug
//! ([`EditError::Internal`]). What triggers them is read from a round once
//! ([`Gone::of`]), by an exhaustive match on [`Key`], so a new kind of
//! slot doesn't compile until it's said whether a rule cares.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use im::Vector;

use super::Lower;
use super::error::EditError;
use super::marks::drop_annots;
use crate::annot::{AnnotId, AnnotKind};
use crate::forms::FieldId;
use crate::model::{
    AnnotBody, AnnotRec, DocState, FieldNode, FormDoc, Generator, Mark, OutlineId, PageId,
};
use crate::prim::{Key, Rec, Round, Val};

/// How many rounds the cascades may take to settle.
pub const MAX_ROUNDS: usize = 8;

/// A cascade rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rule {
    /// A page removed: its annotations go, marks, other tools' comments
    /// and links, and widgets.
    PageAnnots,
    /// An annotation removed: the replies and statuses in its thread go,
    /// ours and other tools', by `/NM` on its page, transitively.
    AnnotThread,
    /// A connector removed: the other end is left without a partner.
    AnnotPartner,
    /// A widget removed: a terminal field with none left, and none on a
    /// page template, goes, and so do the parents it leaves with no kids,
    /// from the roots and the calculation order too, as a save takes them
    /// out of a file (proposal 25).
    WidgetField,
    /// A page removed: the bookmarks going to it go, their kids taking
    /// their places.
    PageBookmarks,
    /// A page removed: a label range starting there starts at the next
    /// page of its range, or goes when none of its pages is left
    /// (proposal 25).
    PageLabels,
    /// A page removed: it leaves the header/footer's pages, which goes
    /// when it's left on none.
    PageGenerators,
}

impl Rule {
    /// Every rule, in the order each round runs them.
    pub const ALL: [Rule; 7] = [
        Rule::PageAnnots,
        Rule::AnnotThread,
        Rule::AnnotPartner,
        Rule::WidgetField,
        Rule::PageBookmarks,
        Rule::PageLabels,
        Rule::PageGenerators,
    ];

    /// Its name, as §6.5 tables it.
    pub fn name(self) -> &'static str {
        match self {
            Rule::PageAnnots => "page.annots",
            Rule::AnnotThread => "annot.thread",
            Rule::AnnotPartner => "annot.partner",
            Rule::WidgetField => "widget.field",
            Rule::PageBookmarks => "page.bookmarks",
            Rule::PageLabels => "page.labels",
            Rule::PageGenerators => "page.generators",
        }
    }

    /// What it does about what a round took away, in `rec`: how many
    /// things it removed or changed.
    fn run(self, rec: &mut Rec, gone: &Gone) -> usize {
        match self {
            Rule::PageAnnots => page_annots(rec, gone),
            Rule::AnnotThread => annot_thread(rec, gone),
            Rule::AnnotPartner => annot_partner(rec, gone),
            Rule::WidgetField => widget_field(rec, gone),
            Rule::PageBookmarks => page_bookmarks(rec, gone),
            Rule::PageLabels => page_labels(rec, gone),
            Rule::PageGenerators => page_generators(rec, gone),
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What a rule did in one step or commit: how many things it removed or
/// changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cascade {
    pub rule: Rule,
    pub objects: usize,
}

/// The cascades of what `lw` wrote, run to a fixed point.
pub(crate) fn settle(lw: &mut Lower) -> Result<Vec<Cascade>, EditError> {
    settle_with(lw, &Rule::ALL)
}

/// [`settle`] with `rules` only: a test switches one off.
pub(crate) fn settle_with(lw: &mut Lower, rules: &[Rule]) -> Result<Vec<Cascade>, EditError> {
    let round = |rec: &mut Rec, gone: &Gone| {
        let done = rules.iter().map(|&rule| (rule, rule.run(rec, gone)));
        done.collect::<Vec<_>>()
    };
    let done = rounds(&mut lw.rec, round).ok_or_else(|| {
        EditError::internal(format!(
            "{}: the cascades didn't settle in {MAX_ROUNDS} rounds",
            lw.op
        ))
    })?;
    let cascades = done.into_iter().filter(|&(_, n)| n > 0);
    Ok(cascades
        .map(|(rule, objects)| Cascade { rule, objects })
        .collect())
}

/// `round` run on what `rec` took away, again on what that took away, and
/// so on, until a round writes nothing: what each rule did, summed. None
/// when that's not in [`MAX_ROUNDS`] rounds.
fn rounds(
    rec: &mut Rec,
    mut round: impl FnMut(&mut Rec, &Gone) -> Vec<(Rule, usize)>,
) -> Option<BTreeMap<Rule, usize>> {
    let mut done: BTreeMap<Rule, usize> = BTreeMap::new();
    // The first round reads what the op wrote, each later one what the
    // rules wrote in the round before.
    for rounds in 0..=MAX_ROUNDS {
        let last = rec.round();
        if last.keys.is_empty() {
            return Some(done);
        }
        if rounds == MAX_ROUNDS {
            break;
        }
        let gone = Gone::of(last, rec);
        for (rule, n) in round(rec, &gone) {
            *done.entry(rule).or_default() += n;
        }
    }
    None
}

/// `cascades` merged by rule, in rule order.
pub(crate) fn merged<'a>(cascades: impl IntoIterator<Item = &'a Cascade>) -> Vec<Cascade> {
    let mut by: BTreeMap<Rule, usize> = BTreeMap::new();
    for c in cascades {
        *by.entry(c.rule).or_default() += c.objects;
    }
    (by.into_iter())
        .map(|(rule, objects)| Cascade { rule, objects })
        .collect()
}

/// What a round of writes took away: what the rules are triggered by.
struct Gone {
    /// The state the round began with.
    before: DocState,
    /// Pages that were there.
    pages: Vec<PageId>,
    /// Annotations that were there, each its page and name.
    annots: Vec<(PageId, String)>,
    /// The fields of widgets that were there.
    widgets: Vec<FieldId>,
    /// Connectors that were there, each with its partner.
    connectors: Vec<(AnnotId, AnnotId)>,
}

impl Gone {
    /// What `round` took away of the state before it, `st` the state
    /// now: each slot it wrote, by kind.
    fn of(round: Round, st: &DocState) -> Gone {
        let mut gone = Gone {
            before: round.before,
            pages: Vec::new(),
            annots: Vec::new(),
            widgets: Vec::new(),
            connectors: Vec::new(),
        };
        for key in &round.keys {
            match *key {
                Key::Page(p) => {
                    if st.def(p).is_none() && gone.before.def(p).is_some() {
                        gone.pages.push(p);
                    }
                }
                Key::Annot(a) => {
                    if st.annot(a).is_none()
                        && let Some(was) = gone.before.annot(a)
                    {
                        gone.annots.push((was.page, was.name()));
                        match &was.body {
                            AnnotBody::Mark(m) => {
                                gone.connectors.extend(m.annot.partner().map(|p| (a, p)));
                            }
                            AnnotBody::Widget(w) => gone.widgets.push(w.field),
                            AnnotBody::Foreign(_) => {}
                        }
                    }
                }
                Key::Order
                | Key::Geom(_)
                | Key::PageAnnots(_)
                | Key::Field(_)
                | Key::Form
                | Key::Bookmark(_)
                | Key::OutlineRoots
                | Key::Labels
                | Key::Generator(_) => {}
            }
        }
        gone
    }
}

/// `page.annots`.
fn page_annots(rec: &mut Rec, gone: &Gone) -> usize {
    let mut n = 0;
    for &page in &gone.pages {
        n += rec.annot_ids_on(page).map_or(0, Vector::len);
        drop_annots(rec, page, |_, _| true);
    }
    n
}

/// `annot.thread`: a reply names the comment it's to by `/NM` on its page
/// (§3.6), so only a name no annotation there has now leaves its thread
/// with nothing to be in; one another comment of the page has, or one that
/// came in with the op, keeps it.
fn annot_thread(rec: &mut Rec, gone: &Gone) -> usize {
    let mut named: HashMap<PageId, HashSet<String>> = HashMap::new();
    for (page, name) in &gone.annots {
        if !name.is_empty() && rec.def(*page).is_some() && rec.annot_by_nm(*page, name).is_none() {
            named.entry(*page).or_default().insert(name.clone());
        }
    }
    let mut n = 0;
    for (page, names) in named {
        let thread = thread_of(rec, page, names);
        n += thread.len();
        drop_annots(rec, page, |id, _| thread.contains(&id));
    }
    n
}

/// `annot.partner`: the other end of a connector that went, when it names
/// that one, is left without a partner.
fn annot_partner(rec: &mut Rec, gone: &Gone) -> usize {
    let mut n = 0;
    for &(end, other) in &gone.connectors {
        let Some(at) = rec.annot(other) else {
            continue;
        };
        let Some(m) = (at.mark_record()).filter(|m| m.annot.partner() == Some(end)) else {
            continue;
        };
        let page = at.page;
        let mut annot = m.annot.clone();
        if let AnnotKind::Connector { partner, .. } = &mut annot.kind {
            *partner = None;
        }
        let mark = Mark {
            annot,
            ..Mark::clone(m)
        };
        let body = AnnotBody::Mark(Arc::new(mark));
        rec.put(Key::Annot(other), Some(Val::Annot(AnnotRec { page, body })));
        n += 1;
    }
    n
}

/// `widget.field`: from the fields whose widgets went, up the tree, each
/// node that shows nowhere now and has no kids goes, out of its parent's
/// kids or the roots, and out of the calculation order.
fn widget_field(rec: &mut Rec, gone: &Gone) -> usize {
    let mut next = gone.widgets.clone();
    let mut removed = HashSet::new();
    while let Some(id) = next.pop() {
        let Some(node) = rec.field(id) else {
            continue;
        };
        let shows = (node.term.as_ref()).is_some_and(|t| !t.widgets.is_empty() || t.outside > 0);
        if shows || !node.kids.is_empty() {
            continue;
        }
        let parent = node.parent;
        rec.put(Key::Field(id), None);
        removed.insert(id);
        if let Some(p) = parent
            && let Some(up) = rec.field(p)
        {
            let mut up = FieldNode::clone(up);
            up.kids.retain(|&kid| kid != id);
            rec.put(Key::Field(p), Some(Val::Field(Arc::new(up))));
            next.push(p);
        }
    }
    if removed.is_empty() {
        return 0;
    }
    let form = rec.form();
    let kept = |ids: &Vector<FieldId>| -> Vector<FieldId> {
        (ids.iter().copied())
            .filter(|id| !removed.contains(id))
            .collect()
    };
    let form = FormDoc {
        roots: kept(&form.roots),
        calc_order: kept(&form.calc_order),
        ..FormDoc::clone(form)
    };
    rec.put(Key::Form, Some(Val::Form(Arc::new(form))));
    removed.len()
}

/// `page.bookmarks`.
fn page_bookmarks(rec: &mut Rec, gone: &Gone) -> usize {
    if gone.pages.is_empty() {
        return 0;
    }
    let gone: HashSet<PageId> = gone.pages.iter().copied().collect();
    promote_kids(rec, &gone)
}

/// `page.labels`: each range that started on a page that went, anchored
/// again ([`crate::labels::anchored`]) by the order the round began with.
fn page_labels(rec: &mut Rec, gone: &Gone) -> usize {
    let Some(specs) = rec.labels().filter(|_| !gone.pages.is_empty()).cloned() else {
        return 0;
    };
    let before = &gone.before;
    let pos = |p| before.pos_of(p);
    let Some((specs, moved)) =
        crate::labels::anchored(&specs, before.order(), pos, |p| rec.def(p).is_some())
    else {
        return 0;
    };
    let labels = (!specs.is_empty()).then(|| Val::Labels(specs.into()));
    rec.put(Key::Labels, labels);
    moved
}

/// `page.generators`: each page that went leaves the pages a generator
/// draws on, when it names them.
fn page_generators(rec: &mut Rec, gone: &Gone) -> usize {
    if gone.pages.is_empty() {
        return 0;
    }
    let went: HashSet<PageId> = gone.pages.iter().copied().collect();
    let generators: Vec<Arc<Generator>> = rec.generators().values().cloned().collect();
    let mut n = 0;
    for g in generators {
        let (left, dropped) = g.on_pages_left(|p| !went.contains(&p));
        if dropped > 0 {
            rec.put(Key::Generator(g.id), left.map(Val::Generator));
            n += dropped;
        }
    }
    n
}

/// The annotations on page `page` in the threads of the comments named
/// `names`: replies and statuses to them, and to those, ours and other
/// tools'; but not those to a name an annotation outside the thread has
/// too.
pub(crate) fn thread_of(
    st: &DocState,
    page: PageId,
    mut names: HashSet<String>,
) -> HashSet<AnnotId> {
    let mut thread = HashSet::new();
    loop {
        let more: Vec<(AnnotId, String)> = (st.annots_on(page))
            .filter(|(id, a)| !thread.contains(id) && a.parent().is_some_and(|p| names.contains(p)))
            .map(|(id, a)| (id, a.name()))
            .collect();
        if more.is_empty() {
            return thread;
        }
        thread.extend(more.iter().map(|&(id, _)| id));
        for (_, name) in more {
            if st.annots_named(page, &name).all(|id| thread.contains(&id)) {
                names.insert(name);
            }
        }
    }
}

/// Takes the bookmarks going to pages in `gone` out of the outline, each
/// one's kids in its place. Puts the bookmarks whose kids change, and the
/// top level when it does. How many went.
fn promote_kids(rec: &mut Rec, gone: &HashSet<PageId>) -> usize {
    /// `kids` with each one going to a page in `gone` replaced by its own
    /// kids, so treated; the ones replaced into `removed`, and those whose
    /// kids change, with their new kids, into `changed`.
    fn kept(
        st: &DocState,
        kids: &Vector<OutlineId>,
        gone: &HashSet<PageId>,
        removed: &mut Vec<OutlineId>,
        changed: &mut Vec<(OutlineId, Vector<OutlineId>)>,
    ) -> Vector<OutlineId> {
        let mut out = Vector::new();
        for &id in kids {
            let Some(b) = st.bookmark(id) else {
                out.push_back(id);
                continue;
            };
            let below = kept(st, &b.kids, gone, removed, changed);
            if b.page().is_some_and(|p| gone.contains(&p)) {
                removed.push(id);
                out.append(below);
            } else {
                if below != b.kids {
                    changed.push((id, below));
                }
                out.push_back(id);
            }
        }
        out
    }
    let (mut removed, mut changed) = (Vec::new(), Vec::new());
    let st: &DocState = rec;
    let roots = kept(st, st.outline_roots(), gone, &mut removed, &mut changed);
    if removed.is_empty() {
        return 0;
    }
    for (id, kids) in changed {
        super::bookmarks::put_kids(rec, Some(id), kids);
    }
    let removed_set: BTreeSet<OutlineId> = removed.iter().copied().collect();
    for &id in &removed_set {
        rec.put(Key::Bookmark(id), None);
    }
    rec.put(
        Key::OutlineRoots,
        Some(crate::prim::Val::OutlineRoots(roots)),
    );
    removed.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::LabelSpec;

    /// A rule that writes something every round never settles: the
    /// rounds stop at [`MAX_ROUNDS`], which [`settle_with`] says is a bug.
    #[test]
    fn rules_that_never_settle_stop() {
        let st = crate::tx::tests::doc_of(1, &[]).state().clone();
        let page = st.page_at(0).unwrap();
        let mut rec = Rec::new(st);
        let labels = |first| {
            let spec = LabelSpec {
                start: page,
                style: crate::labels::Style::Decimal,
                prefix: String::new(),
                first,
            };
            Some(Val::Labels(vec![spec].into()))
        };
        rec.put(Key::Labels, labels(1));
        let mut n = 1;
        let ran = rounds(&mut rec, |rec, _| {
            n += 1;
            rec.put(Key::Labels, labels(n));
            vec![(Rule::PageLabels, 1)]
        });
        assert_eq!(ran, None);
        assert_eq!(n, 1 + MAX_ROUNDS as u32);
        // One that writes nothing settles in a round.
        let mut rec = Rec::new(DocState::builder().build());
        rec.put(Key::Labels, None);
        let ran = rounds(&mut rec, |_, _| vec![(Rule::PageLabels, 0)]);
        assert_eq!(ran, Some(BTreeMap::from([(Rule::PageLabels, 0)])));
    }
}
