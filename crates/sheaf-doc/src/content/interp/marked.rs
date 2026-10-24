//! Marked content where it's drawn, as PDFium marks what it draws
//! (`CPDF_StreamContentParser`'s content marks): the ranges `BMC` and
//! `BDC` open, each with its tag and property list, and what's said of
//! them that later work reads: the marked-content id that ties it to the
//! structure tree, `/ActualText`, and the optional content it's in.

use std::ops::Range;
use std::sync::Arc;

use lopdf::{Dictionary, Object, ObjectId};

use super::UseId;
use crate::content::Props;

/// A marked range, or a marked point (`MP`, `DP`).
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub tag: Vec<u8>,
    /// None for `BMC` and `MP`. A point's named list is
    /// [`Props::Named`] with no dictionary where the resources haven't
    /// got it.
    pub props: Option<Props>,
    /// Its list's `/MCID`, which the structure tree's content items name.
    pub mcid: Option<u32>,
    /// Its list's `/ActualText`, the text it stands for.
    pub actual_text: Option<String>,
    /// Where its tag is `OC`, the optional content group or membership
    /// dictionary its named list is, by reference.
    pub oc: Option<ObjectId>,
    /// The form use whose content it's in; None in the content
    /// interpreted.
    pub stream: Option<UseId>,
    /// Where its `BMC`, `BDC`, `MP` or `DP` is, from its tag.
    pub at: Range<usize>,
}

impl Mark {
    /// The mark `tag` and `props` make, at `at` in `stream`'s content,
    /// `named`, where its list is named, the reference the resources
    /// name it by.
    pub(super) fn new(
        tag: Vec<u8>,
        props: Option<Props>,
        named: Option<&Object>,
        stream: Option<&UseId>,
        at: Range<usize>,
    ) -> Mark {
        let dict: Option<&Dictionary> = props.as_ref().and_then(Props::dict);
        let get = |key: &[u8]| dict.and_then(|d| d.get(key).ok());
        let mcid = get(b"MCID")
            .and_then(|n| n.as_i64().ok())
            .and_then(|n| u32::try_from(n).ok());
        let actual_text = get(b"ActualText")
            .and_then(|t| t.as_str().ok())
            .map(crate::file::text_string);
        let oc = named
            .filter(|_| tag == b"OC")
            .and_then(|r| r.as_reference().ok());
        Mark {
            tag,
            props,
            mcid,
            actual_text,
            oc,
            stream: stream.cloned(),
            at,
        }
    }
}

/// The marked ranges open where something's drawn, across the forms it's
/// drawn in, the innermost first. They're read as PDFium reads them, not
/// strictly as [`content::marked`](crate::content::marked) does:
/// - a `BDC` whose list is a name the resources' `/Properties` haven't
///   got, or neither a name nor a dictionary, opens nothing, so the `EMC`
///   after it ends the range open before it;
/// - an `EMC` with nothing open is passed over;
/// - a range still open where its content ends ends there;
/// - a form starts within the ranges its `Do` is in, which its own `EMC`s
///   can't end.
#[derive(Debug, Clone, Default)]
pub struct Marks(Option<Arc<Node>>);

#[derive(Debug)]
struct Node {
    mark: Mark,
    outer: Marks,
}

impl Marks {
    /// Each range open, the innermost first.
    pub fn iter(&self) -> impl Iterator<Item = &Mark> {
        std::iter::successors(self.0.as_deref(), |node| node.outer.0.as_deref())
            .map(|node| &node.mark)
    }

    pub(super) fn push(&mut self, mark: Mark) {
        let outer = std::mem::take(self);
        *self = Marks(Some(Arc::new(Node { mark, outer })));
    }

    /// The innermost range, now ended, where one is open.
    pub(super) fn pop(&mut self) -> Option<Closed> {
        let node = self.0.take()?;
        self.0.clone_from(&node.outer.0);
        Some(Closed(node))
    }
}

/// A range ended: its mark.
pub(super) struct Closed(Arc<Node>);

impl Closed {
    pub(super) fn mark(&self) -> &Mark {
        &self.0.mark
    }
}

/// Ranges nested as deep as content can nest them are dropped a range at
/// a time, not by recursion.
impl Drop for Marks {
    fn drop(&mut self) {
        let mut next = self.0.take();
        while let Some(node) = next {
            next = Arc::try_unwrap(node)
                .ok()
                .and_then(|mut node| node.outer.0.take());
        }
    }
}
