//! The page tree a Rewrite writes (M10.5 §11.6): balanced, every page at
//! the same depth, no node with more than [`FANOUT`] kids, kids spread
//! evenly. Its nodes carry no inheritable key: the pages carry their own
//! ([`super::pages`]).

use lopdf::{Dictionary, Object};

use super::Cx;
use crate::materialize::key::ObjKey;
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
