//! The `document` interface: the call's document as its transaction holds
//! it, each read recorded as what the call decides by (M10.5 §14.3).

use sheaf_doc::prim::Key;
use sheaf_doc::{AnnotBody, PageId};

use super::api::types as w;
use super::convert::{annot_to_wit, comment_to_wit, outline_item, page_info, version_to_wit};
use crate::runtime::HostState;

impl super::api::document::Host for HostState {
    fn path(&mut self) -> Option<String> {
        self.doc()
            .and_then(|d| d.path.as_ref())
            .map(|p| p.display().to_string())
    }

    fn version(&mut self) -> Option<w::Version> {
        let (tx, base) = self.tx()?;
        Some(version_to_wit(base.snap.instance, tx.base()))
    }

    /// Read: the order, the labels, and each page's content and boxes.
    fn pages(&mut self) -> Vec<w::PageInfo> {
        let Some((tx, _)) = self.tx() else {
            return Vec::new();
        };
        let mut r = tx.reader();
        r.labels();
        let order = r.order().clone();
        let pages: Vec<_> = (order.iter())
            .filter_map(|&p| {
                r.def(p)?;
                Some((p, **r.geom(p)?))
            })
            .collect();
        let st = tx.state();
        (pages.iter())
            .map(|(p, geom)| {
                let label = st.label_of(*p).map(|l| l.to_string()).unwrap_or_default();
                page_info(*p, geom, label)
            })
            .collect()
    }

    /// Read: the top level, and each bookmark.
    fn outline(&mut self) -> Vec<w::OutlineItem> {
        let Some((tx, _)) = self.tx() else {
            return Vec::new();
        };
        let items: Vec<_> = (tx.state().bookmarks_preorder().into_iter())
            .map(|(depth, id, b)| (id, outline_item(depth, id, b)))
            .collect();
        tx.read(Key::OutlineRoots);
        for (id, _) in &items {
            tx.read(Key::Bookmark(*id));
        }
        items.into_iter().map(|(_, item)| item).collect()
    }

    /// Sheaf's marks and other tools' comments; a form field's widget, a
    /// link and an annotation the file hides aren't shown (M10.5 §14.3).
    /// Read: the page's list of annotations, and each one shown.
    fn annotations(&mut self, page: u64) -> Vec<w::AnnotView> {
        let roles = self.roles();
        let Some((tx, base)) = self.tx() else {
            return Vec::new();
        };
        let page = PageId(page);
        let ids = (tx.reader().annot_ids_on(page).cloned()).unwrap_or_default();
        let mut shown = Vec::new();
        for id in ids {
            let view = match tx.state().annot(id).map(|rec| &rec.body) {
                Some(AnnotBody::Mark(m)) => {
                    let author = roles.of(base, &m.annot.author);
                    w::AnnotView::Mark(annot_to_wit(&m.annot, author))
                }
                Some(AnnotBody::Foreign(f)) if !f.summary.is_link() && !f.summary.hidden => {
                    let author = roles.of(base, &f.summary.author);
                    w::AnnotView::Comment(comment_to_wit(id, &f.summary, author))
                }
                _ => continue,
            };
            tx.read(Key::Annot(id));
            shown.push(view);
        }
        shown
    }

    fn selection(&mut self) -> Vec<u64> {
        self.doc()
            .map(|d| d.selection.iter().map(|p| p.0).collect())
            .unwrap_or_default()
    }

    fn current_page(&mut self) -> Option<u64> {
        self.doc()?.current_page.map(|p| p.0)
    }
}
