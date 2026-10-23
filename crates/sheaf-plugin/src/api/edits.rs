//! The `edits` interface: Op JSON lowered into the call's transaction on
//! the plugin's thread (M10.5 §14.3), and the call's edits so far landed
//! by the app's [`Committer`](crate::landing::Committer). Answers are
//! sheaf-edit's replies, as JSON.

use std::time::Instant;

use in_progress::{NoProgress, Progress};
use serde::Serialize;
use sheaf_edit::{Applied, Envelope, Failure, FailureKind, Op, Previewed};

use super::api::{self, types as w};
use super::convert::version_to_wit;
use crate::runtime::HostState;

/// `r` as the API answers it: JSON either way.
fn answer<T: Serialize>(r: Result<T, Failure>) -> Result<String, String> {
    match r {
        Ok(v) => Ok(json(&v)),
        Err(f) => Err(json(&f)),
    }
}

fn json(v: &impl Serialize) -> String {
    serde_json::to_string(v).expect("a reply is JSON")
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure {
        kind: FailureKind::Invalid,
        message: message.into(),
    }
}

impl api::edits::Host for HostState {
    fn submit(&mut self, label: String, ops: String) -> Result<String, String> {
        answer(self.submitted(label, &ops))
    }

    fn preview(&mut self, ops: String) -> Result<String, String> {
        answer(self.previewed(&ops))
    }

    fn commit(&mut self) -> Result<w::Version, String> {
        self.committed().map_err(|f| json(&f))
    }
}

impl HostState {
    /// The ops of `envelope`, as the plugin wrote it: in the version its
    /// manifest says (`edit-schema`), and labelled by the submit alone.
    fn said(&self, envelope: &str) -> Result<Vec<Op>, Failure> {
        let schema = self.edit_schema.as_deref().ok_or_else(|| {
            invalid("the plugin's manifest doesn't say which edits it writes (`edit-schema`)")
        })?;
        let envelope: Envelope =
            serde_json::from_str(envelope).map_err(|e| invalid(e.to_string()))?;
        if envelope.label.is_some() {
            return Err(invalid(
                "a plugin's edits are called as its submit says: the envelope has no label",
            ));
        }
        if envelope.v != schema {
            return Err(invalid(format!(
                "these ops are written in version {}, and the manifest says {schema}",
                envelope.v
            )));
        }
        Ok(envelope.ops)
    }

    /// Ok, unless the call's edits are refused (it answers an undo, an
    /// answer to a change, or a refusal): then the refusal, kept to tell
    /// the plugin once. Its submits, previews and commits all are.
    fn unless_refused(&mut self) -> Result<(), Failure> {
        let Some(why) = self.refusal.clone() else {
            return Ok(());
        };
        if !self.refused.contains(&why) {
            self.refused.push(why.clone());
        }
        Err(Failure {
            kind: FailureKind::Refused,
            message: why,
        })
    }

    /// `ops` lowered into the call's transaction, all or none, what they
    /// made handed out: lowered again where the call's edits land, they
    /// must make it again (M10.5 §8.5). The first submit names the call's
    /// edits.
    fn submitted(&mut self, label: String, ops: &str) -> Result<Applied, Failure> {
        let ops = self.said(ops)?;
        self.unless_refused()?;
        let tracker = self.progress().cloned();
        let (tx, _) = self.tx().ok_or_else(|| invalid("no document is open"))?;
        let p: &dyn Progress = match &tracker {
            Some(t) => t,
            None => &NoProgress,
        };
        let applied = tx.apply_said(ops, p).map_err(|e| Failure::from(&e))?;
        tx.mark_handed_out();
        self.label.get_or_insert(label);
        Ok(Applied::from(applied))
    }

    /// What `ops` would do in the call's transaction, which is left as it
    /// was: refused as its submit would be.
    fn previewed(&mut self, ops: &str) -> Result<Previewed, Failure> {
        let ops = self.said(ops)?;
        self.unless_refused()?;
        let (tx, _) = self.tx().ok_or_else(|| invalid("no document is open"))?;
        let preview = tx.preview_batch(ops, &NoProgress);
        preview.map(Previewed::from).map_err(|e| Failure::from(&e))
    }

    /// The call's edits so far landed now by the app, the call waiting,
    /// which isn't its time: the version they made, which the call's next
    /// edits are made on. With none, the version it reads. Refused when its
    /// edits are.
    fn committed(&mut self) -> Result<w::Version, Failure> {
        self.unless_refused()?;
        let committer = self.doc().and_then(|d| d.committer.clone());
        let (tx, base) = self.tx().ok_or_else(|| invalid("no document is open"))?;
        if tx.is_empty() {
            return Ok(version_to_wit(base.snap.instance, tx.base()));
        }
        let committer = committer
            .ok_or_else(|| invalid("this call's edits land when it returns, not before"))?;
        let tx = self.call_tx.take().expect("the call's transaction");
        let label = self.label.clone().unwrap_or_else(|| self.plugin.clone());
        let started = Instant::now();
        let committed = committer.commit(tx, &label, self.depth);
        self.deadline += started.elapsed();
        match committed {
            Ok(c) => {
                self.base = Some(c.base);
                Ok(version_to_wit(c.instance, c.version))
            }
            Err(e) => {
                // Its edits are gone: the next are made on the document as
                // it is.
                self.base = None;
                Err(Failure::from(&e))
            }
        }
    }
}
