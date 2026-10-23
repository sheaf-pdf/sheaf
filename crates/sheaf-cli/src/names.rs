//! What a document's things are called outside it, as `sheaf edit` and
//! `sheaf inspect` print them (M10.5 §14.2): a page by its position,
//! 1-based; a bookmark by its titles from the top of the outline down; an
//! annotation by its `/NM` and its page. These are what an edit names
//! them by, never a session's ids, which mean nothing outside it.

use std::collections::HashMap;

use serde_json::{Map, Value, json};
use sheaf_doc::tx::{Created, Made};
use sheaf_doc::{AnnotId, DocState, OutlineId, PageId};

/// A state's things, by what a file calls them.
pub struct Names<'a> {
    st: &'a DocState,
    /// Each bookmark's titles, from the top down.
    paths: HashMap<OutlineId, Vec<String>>,
}

impl<'a> Names<'a> {
    pub fn of(st: &'a DocState) -> Names<'a> {
        let mut paths = HashMap::new();
        let mut path: Vec<String> = Vec::new();
        for (depth, id, b) in st.bookmarks_preorder() {
            path.truncate(usize::from(depth));
            path.push(b.title.to_string());
            paths.insert(id, path.clone());
        }
        Names { st, paths }
    }

    /// Page `id`'s position, 1-based.
    pub fn page(&self, id: PageId) -> Option<usize> {
        self.st.pos_of(id).map(|i| i + 1)
    }

    /// Bookmark `id`'s titles, from the top of the outline down.
    pub fn bookmark(&self, id: OutlineId) -> Option<&[String]> {
        self.paths.get(&id).map(Vec::as_slice)
    }

    /// Annotation `id` as `{"nm", "page"}`: what `{"nm": …, "page": {"pos":
    /// …}}` names it by.
    pub fn annot(&self, id: AnnotId) -> Option<Value> {
        let rec = self.st.annot(id)?;
        Some(json!({ "nm": rec.name(), "page": self.page(rec.page) }))
    }

    /// What an op made: `{"page": n}`, `{"bookmark": [titles]}` or
    /// `{"annot": {"nm", "page"}}`; null for what a later op took away.
    pub fn made(&self, made: Made) -> Value {
        let named = match made {
            Made::Page(id) => self.page(id).map(|pos| json!({ "page": pos })),
            Made::Bookmark(id) => self.bookmark(id).map(|path| json!({ "bookmark": path })),
            Made::Annot(id) => self.annot(id).map(|annot| json!({ "annot": annot })),
        };
        named.unwrap_or(Value::Null)
    }

    /// What a transaction's ops made: `{"ops": [[…] per op], "locals":
    /// {"$1": …}}`, each as [`made`](Self::made) names it.
    pub fn created(&self, created: &Created) -> Value {
        let ops: Vec<Value> = (created.ops.iter())
            .map(|made| made.iter().map(|&m| self.made(m)).collect())
            .collect();
        let locals: Map<String, Value> = (created.locals.iter())
            .map(|(local, &m)| (local.to_string(), self.made(m)))
            .collect();
        json!({ "ops": ops, "locals": locals })
    }
}
