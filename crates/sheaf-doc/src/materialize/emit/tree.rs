//! The page tree a save writes (M10.5 §11.6).
//!
//! - **A Rewrite's** is balanced ([`balanced`]): every page at the same
//!   depth, no node with more than [`FANOUT`] kids, kids spread evenly. Its
//!   nodes carry no inheritable key: the pages carry their own
//!   ([`super::pages`]).
//! - **An Append's** is the saved file's, the pages spliced in
//!   ([`spliced`]). A page stays in its node, and a page inserted or moved
//!   joins its predecessor's, so a reorder in a flat tree rewrites one
//!   node. A node grown past max(128, its size at open) splits into
//!   siblings that hold its kids in turn (the root, which keeps its
//!   number, into nodes under it), and a node left with no kids leaves its
//!   parent. Only the nodes whose `/Kids`, `/Count` or `/Parent` changed
//!   are written, each with the keys the file's node held, and each page
//!   under another node than it was is written with all four inheritable
//!   keys. A node is its key wherever it moves: found by its number
//!   through the saved file's map, never by its place in the tree, and a
//!   node split off takes a key past every node key the map has, so a key
//!   an emptied node had is never another's.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{Context as _, Result, bail};
use lopdf::{Dictionary, Object, ObjectId};

use super::Cx;
use crate::materialize::copy::Scope;
use crate::materialize::key::ObjKey;
use crate::materialize::record::{ObjMap, SaveRecord, TreeNode, TreeShape};
use crate::model::PageId;

/// The most kids a node of a written tree has.
pub const FANOUT: usize = 32;

/// A page tree's nodes, in the order the tree is walked ([`ObjKey::PagesNode`]'s
/// order: the root first, each node before the nodes under it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    pub nodes: Vec<PagesNode>,
    /// The node each page is a kid of, by the page's position.
    pub leaf: Vec<u32>,
}

/// A node of a written page tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagesNode {
    /// None for the root.
    pub parent: Option<u32>,
    pub kids: Vec<Kid>,
    /// The pages under it.
    pub count: usize,
}

/// A kid of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kid {
    Node(u32),
    Page(PageId),
}

/// The balanced tree over `pages`, in order. No pages is a root with no
/// kids.
pub fn balanced(pages: &[PageId]) -> Shape {
    let mut height = 1;
    let mut reach = FANOUT;
    while reach < pages.len() {
        height += 1;
        reach = reach.saturating_mul(FANOUT);
    }
    let mut shape = Shape {
        nodes: Vec::new(),
        leaf: vec![0; pages.len()],
    };
    build(&mut shape, pages, 0, height, None);
    shape
}

/// A node over `pages`, the first at position `first`, `height` levels of
/// nodes deep: its number.
fn build(
    shape: &mut Shape,
    pages: &[PageId],
    first: usize,
    height: u32,
    parent: Option<u32>,
) -> u32 {
    let me = shape.nodes.len() as u32;
    shape.nodes.push(PagesNode {
        parent,
        kids: Vec::new(),
        count: pages.len(),
    });
    let kids = if height <= 1 {
        for pos in first..first + pages.len() {
            shape.leaf[pos] = me;
        }
        pages.iter().map(|&p| Kid::Page(p)).collect()
    } else {
        let under = FANOUT.pow(height - 1);
        let groups = pages.len().div_ceil(under).max(1);
        let (size, more) = (pages.len() / groups, pages.len() % groups);
        let mut at = 0;
        let mut kids = Vec::with_capacity(groups);
        for g in 0..groups {
            let len = size + usize::from(g < more);
            let node = build(
                shape,
                &pages[at..at + len],
                first + at,
                height - 1,
                Some(me),
            );
            kids.push(Kid::Node(node));
            at += len;
        }
        kids
    };
    shape.nodes[me as usize].kids = kids;
    me
}

/// `shape`'s nodes, emitted.
pub(crate) fn nodes(cx: &mut Cx, shape: &Shape) {
    for (n, node) in (0..).zip(&shape.nodes) {
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"Pages".to_vec()));
        if let Some(parent) = node.parent {
            dict.set("Parent", cx.placeholder(ObjKey::PagesNode(parent)));
        }
        let kids = (node.kids.iter())
            .map(|kid| {
                cx.placeholder(match *kid {
                    Kid::Node(n) => ObjKey::PagesNode(n),
                    Kid::Page(p) => ObjKey::Page(p),
                })
            })
            .collect::<Vec<_>>();
        dict.set("Kids", kids);
        dict.set("Count", node.count as i64);
        cx.push(ObjKey::PagesNode(n), Object::Dictionary(dict));
    }
}

/// The saved file's page tree with the target's pages placed in it, as an
/// Append writes it ([`spliced`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spliced {
    /// The root first, each node before the nodes under it.
    pub nodes: Vec<SplicedNode>,
    /// The node each page is a kid of, by its place in `nodes`.
    pub leaf: HashMap<PageId, u32>,
    /// The pages under another node than the saved file has them under, or
    /// new to its tree: written, with their `/Parent`.
    pub moved: BTreeSet<PageId>,
}

/// A node of a spliced tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplicedNode {
    /// Its key: the saved file's node's, or past every node key it has.
    pub key: ObjKey,
    /// Its number in the saved file: None for a node split off.
    pub id: Option<ObjectId>,
    /// Its place in [`Spliced::nodes`]: None for the root.
    pub parent: Option<u32>,
    /// Its kids in order, a node by its place in [`Spliced::nodes`].
    pub kids: Vec<Kid>,
    /// The pages under it.
    pub count: usize,
    /// Its size at open ([`TreeNode::size`]).
    pub size: usize,
    /// The primary's node whose other keys it holds ([`TreeNode::from`]).
    pub from: Option<ObjectId>,
    /// Whether it's written: it's new, or its `/Kids`, `/Count` or
    /// `/Parent` aren't the saved file's.
    pub changed: bool,
}

impl Spliced {
    /// The key of the node page `page` is a kid of.
    pub fn parent_of(&self, page: PageId) -> Option<ObjKey> {
        let at = *self.leaf.get(&page)?;
        Some(self.nodes[at as usize].key)
    }

    /// The root's key.
    pub fn root(&self) -> ObjKey {
        self.nodes[0].key
    }

    /// The tree as a record keeps it, each node and page at the number
    /// `number` gives its key: None when it gives none for one.
    pub fn shape(&self, number: &dyn Fn(ObjKey) -> Option<ObjectId>) -> Option<TreeShape> {
        let mut nodes = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let kids = (node.kids.iter())
                .map(|kid| {
                    number(match *kid {
                        Kid::Node(n) => self.nodes[n as usize].key,
                        Kid::Page(p) => ObjKey::Page(p),
                    })
                })
                .collect::<Option<Vec<ObjectId>>>()?;
            nodes.push(TreeNode {
                id: number(node.key)?,
                kids,
                size: node.size,
                from: node.from,
            });
        }
        Some(TreeShape { nodes })
    }
}

/// The key each node of a saved file's page tree is, by its number:
/// what `map` numbers [`ObjKey::PagesNode`]s at. And the key past them.
fn node_keys(map: &ObjMap) -> (HashMap<ObjectId, u32>, u32) {
    let nodes = map
        .keys
        .range(ObjKey::PagesNode(0)..=ObjKey::PagesNode(u32::MAX));
    let mut next = 0;
    let mut by_id = HashMap::new();
    for (key, &id) in nodes {
        if let ObjKey::PagesNode(k) = *key {
            by_id.insert(id, k);
            next = next.max(k.saturating_add(1));
        }
    }
    (by_id, next)
}

/// The key of the root of the page tree of the file `record` says.
pub fn saved_root(record: &SaveRecord) -> Result<ObjKey> {
    let root = record
        .tree
        .nodes
        .first()
        .context("the saved file has no page tree")?;
    let (keys, _) = node_keys(&record.map);
    let k = keys
        .get(&root.id)
        .context("the saved file's page tree has no key")?;
    Ok(ObjKey::PagesNode(*k))
}

/// The node each of `pages` is a kid of in a saved file's tree, `saved`,
/// whose keys `map` has, each page's number in the file `number`: what an
/// Append writes a page under when the order hasn't changed, nor what any
/// page shows (else two pages may trade numbers). None when one of them
/// has no place in the tree (a page its tree lists twice, placed once, or
/// one it hasn't got), which [`spliced`] places.
pub fn parents(
    saved: &TreeShape,
    map: &ObjMap,
    number: &dyn Fn(PageId) -> Option<ObjectId>,
    pages: &BTreeSet<PageId>,
) -> Option<BTreeMap<PageId, ObjKey>> {
    let (keys, _) = node_keys(map);
    let mut at: HashMap<ObjectId, (ObjKey, usize)> = HashMap::new();
    for node in &saved.nodes {
        let key = ObjKey::PagesNode(*keys.get(&node.id)?);
        for &kid in &node.kids {
            at.entry(kid).or_insert((key, 0)).1 += 1;
        }
    }
    (pages.iter())
        .map(|&p| match at.get(&number(p)?) {
            Some(&(key, 1)) => Some((p, key)),
            _ => None,
        })
        .collect()
}

/// The tree of a saved file, `saved`, whose keys `map` has, with the pages
/// `order` placed in it, each page's number in the saved file `number`
/// (None for one it hasn't got). A page whose number its tree lists keeps
/// its node, unless keeping it would put it out of order: the longest run
/// of pages that keep their order stays, and every other page joins the
/// node of the page before it (the first, of the page after it). A page
/// the tree lists twice is placed once. Then nodes left with no kids leave
/// their parents, and a node with more kids than max(128, its size at
/// open) splits ([`split`]). An error when the saved tree isn't a tree, or
/// a node of it has no key.
pub fn spliced(
    saved: &TreeShape,
    map: &ObjMap,
    number: &dyn Fn(PageId) -> Option<ObjectId>,
    order: &[PageId],
) -> Result<Spliced> {
    if saved.nodes.is_empty() {
        bail!("the saved file has no page tree");
    }
    let (keys, mut next) = node_keys(map);
    let count = saved.nodes.len();
    let index: HashMap<ObjectId, usize> = (saved.nodes.iter().enumerate())
        .map(|(i, n)| (n.id, i))
        .collect();
    // The saved tree walked in order: its pages' places (each after so
    // many of its node's kids that are nodes) and its nodes' parents.
    let mut slots: Vec<(usize, usize, ObjectId)> = Vec::new();
    let mut under: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut parent: Vec<Option<usize>> = vec![None; count];
    let mut walked = vec![false; count];
    walked[0] = true;
    let mut stack = vec![(0usize, 0usize)];
    while let Some((i, k)) = stack.pop() {
        let Some(&kid) = saved.nodes[i].kids.get(k) else {
            continue;
        };
        stack.push((i, k + 1));
        match index.get(&kid) {
            Some(&j) => {
                if std::mem::replace(&mut walked[j], true) {
                    bail!(
                        "the saved file's page tree lists node {} {} twice",
                        kid.0,
                        kid.1
                    );
                }
                parent[j] = Some(i);
                under[i].push(j);
                stack.push((j, 0));
            }
            None => slots.push((i, under[i].len(), kid)),
        }
    }
    let mut saved_count = vec![0usize; count];
    for &(node, _, _) in &slots {
        let mut at = Some(node);
        while let Some(i) = at {
            saved_count[i] += 1;
            at = parent[i];
        }
    }
    // Each page's place in the saved tree, the first not taken of its
    // number's.
    let mut by_number: HashMap<ObjectId, Vec<usize>> = HashMap::new();
    for (s, &(_, _, id)) in slots.iter().enumerate() {
        by_number.entry(id).or_default().push(s);
    }
    let mut taken = vec![false; slots.len()];
    let placed: Vec<Option<usize>> = (order.iter())
        .map(|&p| {
            let free = by_number.get(&number(p)?)?.iter().find(|&&s| !taken[s]);
            let s = *free?;
            taken[s] = true;
            Some(s)
        })
        .collect();
    let stays = rising(&placed);
    // The node and the place among its kids each page goes to.
    let mut first = 0;
    while let Some(&j) = (saved.nodes[first].kids.first()).and_then(|kid| index.get(kid))
        && parent[j] == Some(first)
    {
        first = j;
    }
    let bin_of = |s: usize| (slots[s].0, slots[s].1);
    let lead = (0..order.len())
        .find(|&i| stays[i])
        .map(|i| bin_of(placed[i].expect("placed")));
    let mut gaps: Vec<Vec<Vec<PageId>>> = (under.iter())
        .map(|u| vec![Vec::new(); u.len() + 1])
        .collect();
    let mut current = None;
    for (i, &p) in order.iter().enumerate() {
        let bin = match placed[i].filter(|_| stays[i]) {
            Some(s) => bin_of(s),
            None => current.or(lead).unwrap_or((first, 0)),
        };
        current = Some(bin);
        gaps[bin.0][bin.1].push(p);
    }
    let mut work: Vec<Work> = (0..count)
        .map(|i| {
            let mut kids = Vec::new();
            for (g, pages) in gaps[i].iter().enumerate() {
                kids.extend(pages.iter().map(|&p| WorkKid::Page(p)));
                kids.extend(under[i].get(g).map(|&j| WorkKid::Node(j)));
            }
            let node = &saved.nodes[i];
            Work {
                id: Some(node.id),
                parent: parent[i],
                kids,
                size: node.size,
                from: node.from,
            }
        })
        .collect();
    // Emptied nodes leave their parents, those under them first (the saved
    // nodes are in the order the tree is walked).
    let mut gone = vec![false; count];
    for i in (0..count).rev() {
        work[i]
            .kids
            .retain(|kid| !matches!(kid, WorkKid::Node(j) if gone[*j]));
        gone[i] = i != 0 && work[i].kids.is_empty();
    }
    for i in (0..count).rev() {
        if !gone[i] {
            split(&mut work, i);
        }
    }
    // The tree as written, walked from the root.
    let mut walk = Vec::new();
    let mut place = vec![u32::MAX; work.len()];
    let mut stack = vec![0usize];
    while let Some(w) = stack.pop() {
        place[w] = walk.len() as u32;
        walk.push(w);
        for kid in work[w].kids.iter().rev() {
            if let WorkKid::Node(j) = *kid {
                stack.push(j);
            }
        }
    }
    let mut counts = vec![0usize; work.len()];
    for &w in walk.iter().rev() {
        counts[w] = (work[w].kids.iter())
            .map(|kid| match *kid {
                WorkKid::Page(_) => 1,
                WorkKid::Node(j) => counts[j],
            })
            .sum();
    }
    let at_slot: HashMap<PageId, usize> = (order.iter().zip(&placed))
        .filter_map(|(&p, s)| Some((p, (*s)?)))
        .collect();
    let number_of = |kid: &WorkKid, work: &[Work]| match *kid {
        WorkKid::Page(p) => at_slot.get(&p).map(|&s| slots[s].2),
        WorkKid::Node(j) => work[j].id,
    };
    let mut nodes = Vec::with_capacity(walk.len());
    let mut leaf = HashMap::with_capacity(order.len());
    for &w in &walk {
        let node = &work[w];
        let key = match node.id {
            Some(id) => {
                let k = keys.get(&id).with_context(|| {
                    format!("node {} {} of the saved page tree has no key", id.0, id.1)
                })?;
                ObjKey::PagesNode(*k)
            }
            None => {
                next = next.checked_add(1).context("no page tree node key left")?;
                ObjKey::PagesNode(next - 1)
            }
        };
        let changed = match node.id {
            None => true,
            Some(_) => {
                let was = &saved.nodes[w];
                let was_parent = parent[w].map(|i| saved.nodes[i].id);
                let now_parent = node.parent.and_then(|i| work[i].id);
                let kids_kept = was.kids.len() == node.kids.len()
                    && (was.kids.iter().zip(&node.kids))
                        .all(|(&was, kid)| number_of(kid, &work) == Some(was));
                !kids_kept || saved_count[w] != counts[w] || was_parent != now_parent
            }
        };
        let kids = (node.kids.iter())
            .map(|kid| match *kid {
                WorkKid::Page(p) => {
                    leaf.insert(p, place[w]);
                    Kid::Page(p)
                }
                WorkKid::Node(j) => Kid::Node(place[j]),
            })
            .collect();
        nodes.push(SplicedNode {
            key,
            id: node.id,
            parent: node.parent.map(|i| place[i]),
            kids,
            count: counts[w],
            size: node.size,
            from: node.from,
            changed,
        });
    }
    let moved = (order.iter())
        .filter(|p| match at_slot.get(p) {
            Some(&s) => walk[leaf[p] as usize] != slots[s].0,
            None => true,
        })
        .copied()
        .collect();
    Ok(Spliced { nodes, leaf, moved })
}

/// A node of a tree being spliced, by its place among the saved file's
/// nodes, and nodes split off past them.
struct Work {
    id: Option<ObjectId>,
    parent: Option<usize>,
    kids: Vec<WorkKid>,
    size: usize,
    from: Option<ObjectId>,
}

#[derive(Clone, Copy)]
enum WorkKid {
    Node(usize),
    Page(PageId),
}

/// Node `i` of `work` split when it has more kids than max(128, its size
/// at open), into as few as hold them, its kids spread evenly and in
/// order: the root into nodes under it, holding nothing of its own; any
/// other into itself and siblings after it, each holding the keys it
/// holds.
fn split(work: &mut Vec<Work>, i: usize) {
    let most = work[i].size.max(128);
    let len = work[i].kids.len();
    if len <= most {
        return;
    }
    let parts = len.div_ceil(most);
    let (each, more) = (len / parts, len % parts);
    let mut kids = std::mem::take(&mut work[i].kids).into_iter();
    let mut chunks: Vec<Vec<WorkKid>> = (0..parts)
        .map(|c| kids.by_ref().take(each + usize::from(c < more)).collect())
        .collect();
    let root = work[i].parent.is_none();
    let (parent, from) = match root {
        true => (Some(i), None),
        false => (work[i].parent, work[i].from),
    };
    if !root {
        work[i].kids = chunks.remove(0);
    }
    let mut made = Vec::new();
    for chunk in chunks {
        let at = work.len();
        for kid in &chunk {
            if let WorkKid::Node(j) = *kid {
                work[j].parent = Some(at);
            }
        }
        work.push(Work {
            id: None,
            parent,
            size: chunk.len(),
            kids: chunk,
            from,
        });
        made.push(WorkKid::Node(at));
    }
    match parent {
        Some(p) if !root => {
            let after = (work[p].kids.iter())
                .position(|kid| matches!(kid, WorkKid::Node(j) if *j == i))
                .map_or(work[p].kids.len(), |at| at + 1);
            work[p].kids.splice(after..after, made);
        }
        _ => work[i].kids = made,
    }
}

/// Which of the pages `placed` at a saved place stay there: the longest
/// run whose places rise, as they're in order (a page with no place
/// never stays).
fn rising(placed: &[Option<usize>]) -> Vec<bool> {
    // The last of the best run of each length so far, by its index, and
    // each one's predecessor in its run.
    let mut tails: Vec<usize> = Vec::new();
    let mut before: Vec<Option<usize>> = vec![None; placed.len()];
    for (i, s) in placed.iter().enumerate() {
        let Some(s) = *s else {
            continue;
        };
        let len = tails.partition_point(|&t| placed[t].expect("placed") < s);
        before[i] = len.checked_sub(1).map(|l| tails[l]);
        match tails.get_mut(len) {
            Some(t) => *t = i,
            None => tails.push(i),
        }
    }
    let mut stays = vec![false; placed.len()];
    let mut at = tails.last().copied();
    while let Some(i) = at {
        stays[i] = true;
        at = before[i];
    }
    stays
}

/// The nodes of `spliced` that changed, emitted: each with the keys the
/// primary's node it's from holds, but its place in the tree.
pub(crate) fn spliced_nodes(cx: &mut Cx, spliced: &Spliced) {
    let primary = cx.primary.clone();
    let scope = Scope {
        src: &primary,
        copy: 0,
    };
    for node in spliced.nodes.iter().filter(|n| n.changed) {
        let mut dict = Dictionary::new();
        let held = node
            .from
            .and_then(|at| primary.pdf().get_dictionary(at).ok());
        for (k, v) in held.into_iter().flat_map(|d| d.iter()) {
            if !matches!(k.as_slice(), b"Type" | b"Parent" | b"Kids" | b"Count") {
                let v = cx.sym(v, &scope);
                dict.set(k.clone(), v);
            }
        }
        dict.set("Type", Object::Name(b"Pages".to_vec()));
        if let Some(parent) = node.parent {
            let parent = spliced.nodes[parent as usize].key;
            dict.set("Parent", cx.placeholder(parent));
        }
        let kids = (node.kids.iter())
            .map(|kid| {
                cx.placeholder(match *kid {
                    Kid::Node(n) => spliced.nodes[n as usize].key,
                    Kid::Page(p) => ObjKey::Page(p),
                })
            })
            .collect::<Vec<_>>();
        dict.set("Kids", kids);
        dict.set("Count", node.count as i64);
        cx.push(node.key, Object::Dictionary(dict));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages(n: u64) -> Vec<PageId> {
        (0..n).map(PageId).collect()
    }

    /// Each page's depth, walking down from the root, and every node's
    /// kids within the fanout, its count its pages', and its parent the
    /// node that lists it; the walk meets the nodes in their order, and
    /// the pages in theirs.
    fn walk(shape: &Shape, pages: &[PageId]) -> Vec<usize> {
        fn visit(
            shape: &Shape,
            n: u32,
            d: usize,
            seen: &mut Vec<u32>,
            order: &mut Vec<(PageId, usize, u32)>,
        ) -> usize {
            seen.push(n);
            let node = &shape.nodes[n as usize];
            assert!(node.kids.len() <= FANOUT, "node {n}");
            let mut count = 0;
            for kid in &node.kids {
                count += match *kid {
                    Kid::Node(k) => {
                        assert_eq!(shape.nodes[k as usize].parent, Some(n));
                        visit(shape, k, d + 1, seen, order)
                    }
                    Kid::Page(p) => {
                        order.push((p, d, n));
                        1
                    }
                };
            }
            assert_eq!(count, node.count, "node {n}");
            count
        }
        let (mut seen, mut order) = (Vec::new(), Vec::new());
        visit(shape, 0, 1, &mut seen, &mut order);
        assert_eq!(seen, (0..shape.nodes.len() as u32).collect::<Vec<_>>());
        let met: Vec<PageId> = order.iter().map(|&(p, _, _)| p).collect();
        assert_eq!(met, pages);
        let leaves: Vec<u32> = order.iter().map(|&(_, _, n)| n).collect();
        assert_eq!(leaves, shape.leaf);
        order.iter().map(|&(_, d, _)| d).collect()
    }

    #[test]
    fn a_tree_is_balanced_within_its_fanout() {
        for n in [1, 2, 31, 32, 33, 64, 65, 1000, 1024, 1025, 7000] {
            let pages = pages(n);
            let shape = balanced(&pages);
            assert_eq!(shape.nodes[0].parent, None);
            let depth = walk(&shape, &pages);
            assert!(depth.iter().all(|&d| d == depth[0]), "{n}: {depth:?}");
            // As few levels as hold the pages.
            let fewest = (1..).find(|&d| FANOUT.pow(d) >= n as usize).unwrap() as usize;
            assert_eq!(depth[0], fewest, "{n}");
        }
        // Spread evenly: 33 pages are two leaves of 17 and 16.
        let shape = balanced(&pages(33));
        let sizes: Vec<usize> = shape.nodes[1..].iter().map(|n| n.count).collect();
        assert_eq!(sizes, [17, 16]);
        // 7,000 pages: 7 nodes under the root, each of 1,000 in 32 leaves.
        let shape = balanced(&pages(7000));
        assert_eq!(shape.nodes[0].kids.len(), 7);
        assert_eq!(shape.nodes.len(), 1 + 7 + 7 * 32);
    }

    /// A saved tree: each node `(number, kids)` in the order the tree is
    /// walked, a kid under 100 a node and any other a page; each node's
    /// size at open its kids, its keys its own. Its map keys each node by
    /// its place.
    fn saved(nodes: &[(u32, &[u32])]) -> (TreeShape, ObjMap) {
        let nodes: Vec<TreeNode> = (nodes.iter())
            .map(|&(n, kids)| TreeNode {
                id: (n, 0),
                kids: kids.iter().map(|&k| (k, 0)).collect(),
                size: kids.len(),
                from: Some((n, 0)),
            })
            .collect();
        let keys = (0..).zip(&nodes).map(|(k, n)| (ObjKey::PagesNode(k), n.id));
        let map = ObjMap {
            identity: None,
            keys: keys.collect(),
        };
        (TreeShape { nodes }, map)
    }

    /// Page `PageId(n)` is object `n` of the saved file; one past 9,999 it
    /// hasn't got.
    fn at(p: PageId) -> Option<ObjectId> {
        (p.0 < 10_000).then_some((p.0 as u32, 0))
    }

    fn ids(ns: impl IntoIterator<Item = u32>) -> Vec<PageId> {
        ns.into_iter().map(|n| PageId(u64::from(n))).collect()
    }

    /// The pages under each node, walked in order.
    fn walked(s: &Spliced) -> Vec<PageId> {
        fn visit(s: &Spliced, n: u32, out: &mut Vec<PageId>) {
            for kid in &s.nodes[n as usize].kids {
                match *kid {
                    Kid::Node(k) => {
                        assert_eq!(s.nodes[k as usize].parent, Some(n));
                        visit(s, k, out);
                    }
                    Kid::Page(p) => {
                        assert_eq!(s.leaf[&p], n);
                        out.push(p);
                    }
                }
            }
        }
        let mut out = Vec::new();
        visit(s, 0, &mut out);
        out
    }

    /// A reorder in a flat tree of 7,000 pages writes the one node, and no
    /// page; the same order writes nothing.
    #[test]
    fn a_reorder_in_a_flat_tree_writes_one_node() {
        let kids: Vec<u32> = (100..7100).collect();
        let (tree, map) = saved(&[(1, &kids)]);
        let mut order = ids(100..7100);
        let same = spliced(&tree, &map, &at, &order).unwrap();
        assert!(same.nodes.iter().all(|n| !n.changed) && same.moved.is_empty());
        let moved = order.remove(5000);
        order.insert(10, moved);
        let s = spliced(&tree, &map, &at, &order).unwrap();
        assert_eq!(s.nodes.len(), 1);
        assert!(s.nodes[0].changed);
        assert_eq!(
            (s.nodes[0].key, s.nodes[0].count),
            (ObjKey::PagesNode(0), 7000)
        );
        assert!(s.moved.is_empty());
        assert_eq!(walked(&s), order);
    }

    /// A page inserted joins the node of the page before it, and one
    /// inserted first the node of the page after it: only that node and
    /// the nodes above it are written, and only the new page.
    #[test]
    fn an_insert_joins_its_predecessor() {
        let (tree, map) = saved(&[(1, &[2, 3]), (2, &[100, 101, 102]), (3, &[103, 104, 105])]);
        let new = PageId(10_000);
        let mut order = ids(100..106);
        order.insert(4, new);
        let s = spliced(&tree, &map, &at, &order).unwrap();
        assert_eq!(walked(&s), order);
        assert_eq!(s.parent_of(new), Some(ObjKey::PagesNode(2)));
        let changed: Vec<ObjKey> = s
            .nodes
            .iter()
            .filter(|n| n.changed)
            .map(|n| n.key)
            .collect();
        assert_eq!(changed, [ObjKey::PagesNode(0), ObjKey::PagesNode(2)]);
        assert_eq!(s.moved, BTreeSet::from([new]));
        let mut first = ids(100..106);
        first.insert(0, new);
        let s = spliced(&tree, &map, &at, &first).unwrap();
        assert_eq!(walked(&s), first);
        assert_eq!(s.parent_of(new), Some(ObjKey::PagesNode(1)));
    }

    /// A page moved to another node is written under it; the rest stay.
    #[test]
    fn a_page_moved_across_nodes_is_written_under_its_new_node() {
        let (tree, map) = saved(&[(1, &[2, 3]), (2, &[100, 101, 102]), (3, &[103, 104, 105])]);
        let mut order = ids(100..106);
        let first = order.remove(0);
        order.push(first);
        let s = spliced(&tree, &map, &at, &order).unwrap();
        assert_eq!(walked(&s), order);
        assert_eq!(s.parent_of(first), Some(ObjKey::PagesNode(2)));
        assert_eq!(s.moved, BTreeSet::from([first]));
        // Both leaves; the root's kids and count are as they were.
        let changed: Vec<ObjKey> = s
            .nodes
            .iter()
            .filter(|n| n.changed)
            .map(|n| n.key)
            .collect();
        assert_eq!(changed, [ObjKey::PagesNode(1), ObjKey::PagesNode(2)]);
    }

    /// A node splits only past max(128, its size at open): into itself and
    /// a sibling after it, which takes a key past every node key the map
    /// has and the keys its node holds; the root, which keeps its key, into
    /// nodes under it.
    #[test]
    fn a_node_splits_past_max_128_and_its_size() {
        let a: Vec<u32> = (100..228).collect();
        let (tree, map) = saved(&[(1, &[2, 3]), (2, &a), (3, &[300])]);
        // 128 kids, as at open: no split.
        let mut order = ids(100..228);
        order.push(PageId(300));
        let s = spliced(&tree, &map, &at, &order).unwrap();
        assert_eq!(s.nodes.len(), 3);
        // One more is past 128.
        order.insert(64, PageId(10_000));
        let s = spliced(&tree, &map, &at, &order).unwrap();
        assert_eq!(walked(&s), order);
        let keys: Vec<ObjKey> = s.nodes.iter().map(|n| n.key).collect();
        let [root, a, more, b] = keys[..] else {
            panic!("{keys:?}")
        };
        assert_eq!([root, a, b], [0, 1, 2].map(ObjKey::PagesNode));
        assert_eq!(more, ObjKey::PagesNode(3));
        let node = |k: ObjKey| s.nodes.iter().find(|n| n.key == k).unwrap();
        assert_eq!((node(a).count, node(more).count), (65, 64));
        assert_eq!((node(more).id, node(more).from), (None, Some((2, 0))));
        assert_eq!(node(more).size, 64);
        assert!(node(root).changed && node(a).changed && node(more).changed);
        assert!(!node(b).changed);
        // The pages that went to the new node are written; those staying
        // with theirs aren't, but the one inserted.
        let went: BTreeSet<PageId> = ids(164..228).into_iter().collect();
        let mut moved = went.clone();
        moved.insert(PageId(10_000));
        assert_eq!(s.moved, moved);

        // A flat root of 200 grown past its size at open splits into nodes
        // under it, and keeps its key.
        let kids: Vec<u32> = (100..300).collect();
        let (tree, map) = saved(&[(1, &kids)]);
        let mut order = ids(100..300);
        order.push(PageId(10_000));
        let s = spliced(&tree, &map, &at, &order).unwrap();
        assert_eq!(walked(&s), order);
        assert_eq!(s.nodes.len(), 3);
        assert_eq!(s.root(), ObjKey::PagesNode(0));
        assert_eq!((s.nodes[1].count, s.nodes[2].count), (101, 100));
        assert_eq!(
            (s.nodes[1].from, s.nodes[1].key),
            (None, ObjKey::PagesNode(1))
        );
        assert_eq!(s.moved.len(), 201);
    }

    /// A node left with no pages leaves its parent, and its key is no
    /// node's; the root stays, with no kids when no page is left.
    #[test]
    fn an_emptied_node_leaves_its_parent() {
        let (tree, map) = saved(&[(1, &[2, 3]), (2, &[100, 101]), (3, &[102, 103])]);
        let order = ids([100, 101]);
        let s = spliced(&tree, &map, &at, &order).unwrap();
        assert_eq!(walked(&s), order);
        let keys: Vec<ObjKey> = s.nodes.iter().map(|n| n.key).collect();
        assert_eq!(keys, [ObjKey::PagesNode(0), ObjKey::PagesNode(1)]);
        assert_eq!(s.nodes[0].kids, [Kid::Node(1)]);
        assert!(s.nodes[0].changed && !s.nodes[1].changed);
        let s = spliced(&tree, &map, &at, &[]).unwrap();
        assert_eq!(s.nodes.len(), 1);
        assert!(s.nodes[0].kids.is_empty() && s.nodes[0].changed);
    }

    /// A node is the key the map gives its number, wherever the tree is
    /// walked, and a node split off takes a key past every node key the
    /// map has, used or not.
    #[test]
    fn nodes_are_their_keys_by_number() {
        let (tree, mut map) = saved(&[(1, &[2, 3]), (2, &[100]), (3, &[101])]);
        map.keys = BTreeMap::from([
            (ObjKey::PagesNode(7), (1, 0)),
            (ObjKey::PagesNode(2), (2, 0)),
            (ObjKey::PagesNode(5), (3, 0)),
            (ObjKey::PagesNode(9), (50, 0)),
        ]);
        let s = spliced(&tree, &map, &at, &ids([100, 101])).unwrap();
        let keys: Vec<ObjKey> = s.nodes.iter().map(|n| n.key).collect();
        assert_eq!(keys, [7, 2, 5].map(ObjKey::PagesNode));
        // Grown past max(128, 1).
        let mut order = ids([100]);
        order.extend((0..128).map(|i| PageId(10_000 + i)));
        order.push(PageId(101));
        let s = spliced(&tree, &map, &at, &order).unwrap();
        assert_eq!(walked(&s), order);
        let new: Vec<ObjKey> = s
            .nodes
            .iter()
            .filter(|n| n.id.is_none())
            .map(|n| n.key)
            .collect();
        assert_eq!(new, [ObjKey::PagesNode(10)]);
        // A node with no key, or listed twice, is an error.
        map.keys.remove(&ObjKey::PagesNode(5));
        assert!(spliced(&tree, &map, &at, &order).is_err());
        let (twice, map) = saved(&[(1, &[2, 2]), (2, &[100])]);
        assert!(spliced(&twice, &map, &at, &ids([100])).is_err());
    }

    /// A page its tree lists twice is placed once; the other listing goes.
    #[test]
    fn a_page_listed_twice_is_placed_once() {
        let (tree, map) = saved(&[(1, &[100, 101, 100])]);
        let order = ids([100, 101]);
        let s = spliced(&tree, &map, &at, &order).unwrap();
        assert_eq!(walked(&s), order);
        assert!(s.nodes[0].changed);
    }

    #[test]
    fn no_pages_is_a_root_with_no_kids() {
        let shape = balanced(&[]);
        assert_eq!(
            shape.nodes,
            [PagesNode {
                parent: None,
                kids: Vec::new(),
                count: 0
            }]
        );
        assert!(shape.leaf.is_empty());
    }
}
