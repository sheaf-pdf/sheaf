//! A transaction made on a version that's no longer the current one,
//! landed on the current one (M10.5 §8.5). What its caller based its ops
//! on must hold the same values there; then its changes are put there,
//! checked by value ([`Stale::Validate`]), or its ops are lowered there
//! again ([`Stale::Relower`]), making what they made first once that was
//! handed out. What it depended on that changed since is a
//! [`EditError::Conflict`], never repaired: nothing changes, and the
//! reader is told which edit changed it.

use std::collections::{BTreeSet, HashSet};

use super::{DocState, Document, GroupId, Step, Version, VersionId};
use crate::describe::describe;
use crate::effects::StepEffects;
use crate::prim::{Key, Rec, Val, same};
use crate::tx::{Base, EditError, Made, Stale, TxParts, cascade};

impl Document {
    /// `parts`, a transaction made on version `parts.base`, as it lands on
    /// the current version: as it is when that's the one it was made on,
    /// else as `stale` says, once its decision reads hold there. Its
    /// caller says which version it made it on (`base`, the current one
    /// unless it says another): one that says another than it was, or
    /// commits a transaction made on another document, such as the one a
    /// reopen replaced, has a bug.
    pub(super) fn onto_cursor(
        &self,
        parts: TxParts,
        base: Base,
        stale: Stale,
    ) -> Result<TxParts, EditError> {
        if parts.instance != self.instance {
            return Err(EditError::internal(
                "a transaction made on another document committed on this one",
            ));
        }
        let made = parts.base;
        let said = match base {
            Base::At(v) => v,
            Base::Current => self.cursor,
        };
        if said != made {
            return Err(EditError::internal(format!(
                "a transaction made on version {} committed as made on {}",
                made.0, said.0
            )));
        }
        if made == self.cursor {
            return Ok(parts);
        }
        for (&key, val) in &parts.decided {
            self.holds(key, val.as_ref(), made)?;
        }
        match stale {
            Stale::Validate => self.validate(parts),
            Stale::Relower => self.relower(parts),
        }
    }

    /// `parts`' prims put on the current version ([`merge`](Self::merge)),
    /// when every slot its ops read holds the same value there.
    fn validate(&self, mut parts: TxParts) -> Result<TxParts, EditError> {
        for (&key, val) in parts.seen.iter() {
            self.holds(key, val.as_ref(), parts.base)?;
        }
        let steps = std::mem::take(&mut parts.steps);
        (parts.state, parts.steps) = self.merge(steps, parts.base, None, &parts.state)?;
        Ok(parts)
    }

    /// The prims of `steps`, which made `theirs` on version `made`, put on
    /// the current version, when each one's before holds there as the
    /// prims before it left it: the state they make, and the steps,
    /// each one's line said again against the document it now follows and
    /// what it did judged again. Then the cascades must find nothing to do
    /// about what they wrote ([`cascade::unsettled`]), as on the version
    /// they were made on, and what they wrote must be unique where it must
    /// be and name pages there are ([`DocState::unique_after`],
    /// [`DocState::resolves_after`]): a page they delete that another edit
    /// has since given a connector, or a second header/footer, is a
    /// Conflict. Group `ours`' versions, which a revert takes back, are
    /// never the edit a Conflict names.
    pub(super) fn merge(
        &self,
        mut steps: Vec<Step>,
        made: VersionId,
        ours: Option<GroupId>,
        theirs: &DocState,
    ) -> Result<(DocState, Vec<Step>), EditError> {
        let cursor = self.state();
        let mut written = BTreeSet::new();
        let mut rec = Rec::new(cursor.clone());
        for step in &mut steps {
            let before = DocState::clone(&rec);
            for p in step.prims.iter() {
                if !same(rec.get(&p.key).as_ref(), p.before.as_ref()) {
                    return Err(self.conflict(p.key, made, ours));
                }
                written.insert(p.key);
                rec.put(p.key, p.after.clone());
            }
            step.line = describe(&step.op, &before);
            step.effects = StepEffects::of(&before, &rec, &step.prims);
        }
        let state = DocState::clone(&rec);
        let broken = match cascade::unsettled(&mut rec) {
            Some(key) => Err(key),
            None => (state.unique_after(&written, [cursor, theirs]))
                .and_then(|()| state.resolves_after(&written)),
        };
        broken.map_err(|key| self.conflict(key, made, ours))?;
        Ok((state, steps))
    }

    /// `parts`' ops lowered again on the current version, each drawing the
    /// ids it drew first. An op that resolved a position read the order,
    /// which must be as it was: the position would name another page now.
    /// What they made, once handed out ([`Tx::mark_handed_out`]), they must
    /// make again, each the same thing under the same id and local, or it's
    /// a Conflict: over the first slot they read that changed since, by
    /// the edit that changed it; else, when what changed isn't a slot they
    /// record reading (the outline a bookmark's span is read from), over
    /// the first slot they write, by no edit named.
    ///
    /// [`Tx::mark_handed_out`]: crate::tx::Tx::mark_handed_out
    fn relower(&self, parts: TxParts) -> Result<TxParts, EditError> {
        if parts.positional() {
            let order = parts.base_state.get(&Key::Order);
            self.holds(Key::Order, order.as_ref(), parts.base)?;
        }
        let handed = (parts.handed_out).then(|| (parts.created.clone(), parts.seen.clone()));
        let made = parts.base;
        let again = parts.relowered(self.cursor, self.state().clone())?;
        let Some((created, seen)) = handed.filter(|(created, _)| *created != again.created) else {
            return Ok(again);
        };
        let changed =
            (seen.iter()).find(|(key, val)| !same(self.state().get(key).as_ref(), val.as_ref()));
        if let Some((&key, _)) = changed {
            return Err(self.conflict(key, made, None));
        }
        let wrote = (again.steps.iter())
            .flat_map(|s| s.prims.iter())
            .map(|p| p.key);
        match wrote.chain(created.made().map(made_key)).next() {
            Some(key) => Err(EditError::Conflict { key, by: None }),
            None => Err(EditError::internal(
                "lowered again, it made other things, though neither made any",
            )),
        }
    }

    /// Whether slot `key` holds `val` on the current version, as it did on
    /// version `made`, where a transaction read it or wrote over it: else
    /// a Conflict.
    fn holds(&self, key: Key, val: Option<&Val>, made: VersionId) -> Result<(), EditError> {
        match same(self.state().get(&key).as_ref(), val) {
            true => Ok(()),
            false => Err(self.conflict(key, made, None)),
        }
    }

    /// A Conflict over slot `key` for a transaction made on version
    /// `made`: by the newest version on the current one's line, up to
    /// where `made`'s meets it, whose steps changed `key`, other than group
    /// `ours`'; by none when the change was an undo. Version `made` may no
    /// longer be kept (pruned, or put in place of by an amend): then the
    /// whole line is looked at.
    pub(super) fn conflict(&self, key: Key, made: VersionId, ours: Option<GroupId>) -> EditError {
        let mut theirs = HashSet::new();
        let mut at = Some(made).filter(|v| self.versions.contains_key(v));
        while let Some(id) = at {
            theirs.insert(id);
            at = self.versions[&id].parent;
        }
        let ours = |v: &Version| ours.is_some_and(|g| v.group.as_ref().is_some_and(|t| t.0 == g));
        let mut at = Some(self.cursor);
        while let Some(id) = at.filter(|id| !theirs.contains(id)) {
            let v = &self.versions[&id];
            if v.keys.contains(&key) && !ours(v) {
                let by = Some(v.label.as_str().into());
                return EditError::Conflict { key, by };
            }
            at = v.parent;
        }
        EditError::Conflict { key, by: None }
    }
}

/// The slot that holds what an op made.
fn made_key(m: Made) -> Key {
    match m {
        Made::Page(id) => Key::Page(id),
        Made::Bookmark(id) => Key::Bookmark(id),
        Made::Annot(id) => Key::Annot(id),
    }
}
