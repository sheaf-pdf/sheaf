//! What sheaf answers a caller that hands it ops (M10.5 §14.3), as JSON:
//! what a plugin's submit applied, what a preview would do, and why one
//! failed. Answers, never read from a file, so they aren't in the
//! envelope's schema.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{AnnotId, Local, OutlineId, PageId};

/// Something an op made, which a local can name: `{"page": 12}`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Made {
    Page(PageId),
    Bookmark(OutlineId),
    Annot(AnnotId),
}

/// What a change needs of whoever makes it (§6.7).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Cap {
    /// What any plugin may do: pages moved, turned, deleted or copied,
    /// labels, bookmarks, sheaf's own marks.
    Plugin,
    /// What changes or removes content the user didn't make in sheaf: the
    /// user approves it.
    Approval,
    /// What only the user and the host's own jobs do: content from files,
    /// recognized text, imported comments' names and dates.
    User,
}

impl std::fmt::Display for Cap {
    /// The refusal of a change that needs it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Cap::Plugin => "refused: a plugin's edits are its own to make",
            Cap::Approval => "refused: that needs your approval",
            Cap::User => "refused: only you can make that change",
        })
    }
}

/// What a submit applied: what each op made, in order, what each local
/// it gave names, and its lines, as History says them. The ids are the
/// ones the call's commit keeps, or it fails.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Applied {
    pub created: Vec<Vec<Made>>,
    pub locals: BTreeMap<Local, Made>,
    pub lines: Vec<String>,
}

/// What ops would do, applied now, with nothing applied: what a submit of
/// them would answer, what each op needs of whoever makes it, in order,
/// and whether they'd remove something that's there (a page, a comment,
/// a bookmark, a layer). The ids they'd make are drawn, never handed out
/// again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Previewed {
    pub lines: Vec<String>,
    pub created: Vec<Vec<Made>>,
    pub locals: BTreeMap<Local, Made>,
    pub caps: Vec<Cap>,
    pub removes: bool,
}

/// Why ops weren't applied, or a commit didn't land: nothing changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub kind: FailureKind,
    /// The reason, as sheaf tells the user.
    pub message: String,
}

/// What kind of failure: one for each way an edit fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FailureKind {
    /// Something an op names isn't in the document.
    NotFound,
    /// An op can't be done as said.
    Invalid,
    /// A name an op gives fits more than one thing.
    Ambiguous,
    /// A page's content an op needs can't be read or changed as it asks.
    Content,
    /// The caller may not make the change.
    Refused,
    /// The file is still opening.
    NotReady,
    Cancelled,
    /// What the ops were based on changed since: another edit changed the
    /// same data.
    Conflict,
    /// The edits it adds to were undone.
    GroupEnded,
    /// History is damaged.
    Mismatch,
    /// A bug in sheaf.
    Internal,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A reply as JSON, and back.
    fn round_trip<T>(reply: &T, json: serde_json::Value)
    where
        T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        assert_eq!(serde_json::to_value(reply).unwrap(), json);
        assert_eq!(&serde_json::from_value::<T>(json).unwrap(), reply);
    }

    #[test]
    fn replies_round_trip_through_json() {
        let applied = Applied {
            created: vec![vec![Made::Annot(AnnotId(7))], vec![Made::Page(PageId(12))]],
            locals: BTreeMap::from([
                (Local(1), Made::Annot(AnnotId(7))),
                (Local(10), Made::Bookmark(OutlineId(3))),
            ]),
            lines: vec!["Add a note on page 1".into()],
        };
        let applied_json = json!({
            "created": [[{"annot": 7}], [{"page": 12}]],
            "locals": {"$1": {"annot": 7}, "$10": {"bookmark": 3}},
            "lines": ["Add a note on page 1"],
        });
        round_trip(&applied, applied_json);
        let previewed = Previewed {
            lines: vec!["Delete page 2".into()],
            created: vec![Vec::new()],
            locals: BTreeMap::new(),
            caps: vec![Cap::Plugin, Cap::Approval],
            removes: true,
        };
        let previewed_json = json!({
            "lines": ["Delete page 2"],
            "created": [[]],
            "locals": {},
            "caps": ["plugin", "approval"],
            "removes": true,
        });
        round_trip(&previewed, previewed_json);
        let failure = Failure {
            kind: FailureKind::NotFound,
            message: "delete-pages: page 9 of 3 isn't there".into(),
        };
        let failure_json = json!({
            "kind": "not-found",
            "message": "delete-pages: page 9 of 3 isn't there",
        });
        round_trip(&failure, failure_json);
    }

    #[test]
    fn a_reply_refuses_a_field_it_does_not_know() {
        let read = |json| serde_json::from_value::<Failure>(json).map_err(|e| e.to_string());
        let e = read(json!({"kind": "invalid", "message": "", "why": 1})).unwrap_err();
        assert!(e.contains("unknown field `why`"), "{e}");
        let e = read(json!({"kind": "wrong", "message": ""})).unwrap_err();
        assert!(e.contains("unknown variant `wrong`"), "{e}");
        let made = serde_json::from_value::<Made>(json!({"field": 1}));
        assert!(made.is_err());
    }
}
