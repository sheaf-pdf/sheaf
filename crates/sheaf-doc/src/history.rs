//! How much undo history to keep.
//!
//! History is a tree of versions, each with the edits that made it and
//! what they changed, as prims (see `model::Document` and `prim`). A
//! policy weighs what it holds (M10.5 §8.7). Trimming drops snapshots of
//! the document: a version whose snapshot is dropped is rebuilt when it's
//! needed, by applying the prims from the nearest version that kept one.
//! That frees only the snapshots' bytes. What the steps hold is freed when
//! versions are pruned, past [`MAX_VERSIONS`] or when the policy says:
//! versions on other branches first, the oldest first, then the oldest on
//! the current line, the first version kept moving on. That one keeps its
//! steps' lines but not their prims, since nothing is rebuilt from before
//! it. The saved version may be pruned too, and what a save needs of it
//! stays beside the history: its state, its record and the keys changed
//! since, so the next save still appends. A version being saved never is.
//! Pruning frees the files only pruned versions show pages of too: a
//! source's blob goes with the last record holding it, and an engine
//! opened on it keeps only its bytes, until slice 13's engine pool closes
//! it. A source's bytes are never step bytes; the store counts each blob
//! once.

/// Versions kept, with or without snapshots. Past this, versions on other
/// branches go first, then the oldest on the current line.
pub const MAX_VERSIONS: usize = 1000;

/// What a policy gets to look at. Snapshot and step bytes are estimates;
/// source bytes are the blobs' lengths.
#[derive(Debug, Clone)]
pub struct HistoryStats {
    /// Snapshots currently held (the current version's included).
    pub versions: usize,
    /// Index of the current version's snapshot: the last.
    pub cursor: usize,
    /// How many snapshots, from the front of `snapshot_bytes`, may be
    /// dropped. The rest must stay: the current version's, the first
    /// kept, the saved one and those being saved.
    pub droppable: usize,
    /// Estimated bytes each snapshot holds that no other version does, in
    /// the order they'd be dropped: those that may be first, then those
    /// that must stay.
    pub snapshot_bytes: Vec<usize>,
    /// Estimated bytes all versions' steps hold: their edits and what they
    /// changed. Dropping snapshots doesn't free these; pruning versions
    /// does.
    pub step_bytes: usize,
    /// Bytes of every blob the document's store holds, each counted once:
    /// the opened file, inserted ones, derived copies and saved ones,
    /// whatever holds them. Trimming frees none of it; pruning, what only
    /// the pruned versions held.
    pub blob_bytes: usize,
    /// Bytes of the blobs only versions off the current line hold (inserts
    /// that were undone), each counted once: the part of `blob_bytes`
    /// pruning those versions would free.
    pub undone_source_bytes: usize,
}

impl HistoryStats {
    /// What a policy weighs: snapshots, steps and undone sources. Not
    /// `blob_bytes`, most of which is the files the document shows, which
    /// no pruning frees.
    pub fn total_bytes(&self) -> usize {
        self.snapshot_bytes.iter().sum::<usize>() + self.step_bytes + self.undone_source_bytes
    }

    /// What dropping every snapshot that may be dropped would leave: the
    /// snapshots that must stay, the steps and the undone sources. Only
    /// pruning versions makes it less.
    pub fn untrimmable_bytes(&self) -> usize {
        let kept: usize = self.snapshot_bytes[self.droppable..].iter().sum();
        kept + self.step_bytes + self.undone_source_bytes
    }
}

pub trait HistoryPolicy: Send + Sync {
    /// Whether the policy might act on history holding `snapshots`
    /// snapshots and `bytes` in all ([`HistoryStats::total_bytes`]): when
    /// it can't, the document doesn't work out the stats it would weigh,
    /// so an edit needn't walk the history. One that doesn't say might.
    fn could_act(&self, snapshots: usize, bytes: usize) -> bool {
        let _ = (snapshots, bytes);
        true
    }

    /// How many snapshots to drop, from the front of `snapshot_bytes`. The
    /// document never drops more than it may ([`HistoryStats::droppable`]).
    fn trim(&self, stats: &HistoryStats) -> usize;

    /// Whether to prune a version (see the module's notes on the order),
    /// before snapshots are trimmed. Asked again after each, until it says
    /// no, or nothing more may go. One that doesn't say never prunes.
    fn prunes(&self, stats: &HistoryStats) -> bool {
        let _ = stats;
        false
    }
}

/// Keep at most this many snapshots.
pub struct FixedCount(pub usize);

impl HistoryPolicy for FixedCount {
    fn could_act(&self, snapshots: usize, _bytes: usize) -> bool {
        snapshots > self.0.max(1)
    }

    fn trim(&self, stats: &HistoryStats) -> usize {
        stats.versions.saturating_sub(self.0.max(1))
    }
}

/// Keep history within this many bytes. The oldest snapshots are dropped
/// until the estimate fits; when what dropping them all would leave is
/// over it ([`HistoryStats::untrimmable_bytes`]), versions are pruned
/// first, until it isn't. So what history holds fits, unless the
/// snapshots that must stay are over it on their own.
pub struct MemoryBudget(pub usize);

impl HistoryPolicy for MemoryBudget {
    fn could_act(&self, _snapshots: usize, bytes: usize) -> bool {
        bytes > self.0
    }

    fn trim(&self, stats: &HistoryStats) -> usize {
        let mut total = stats.total_bytes();
        let mut drop = 0;
        while total > self.0 && drop < stats.droppable {
            total -= stats.snapshot_bytes[drop];
            drop += 1;
        }
        drop
    }

    fn prunes(&self, stats: &HistoryStats) -> bool {
        stats.untrimmable_bytes() > self.0
    }
}

/// Drop enough to satisfy every policy, and prune when any says.
pub struct AllOf(pub Vec<Box<dyn HistoryPolicy>>);

impl HistoryPolicy for AllOf {
    fn could_act(&self, snapshots: usize, bytes: usize) -> bool {
        self.0.iter().any(|p| p.could_act(snapshots, bytes))
    }

    fn trim(&self, stats: &HistoryStats) -> usize {
        self.0.iter().map(|p| p.trim(stats)).max().unwrap_or(0)
    }

    fn prunes(&self, stats: &HistoryStats) -> bool {
        self.0.iter().any(|p| p.prunes(stats))
    }
}

/// 200 snapshots and 256 MB.
pub fn default_policy() -> Box<dyn HistoryPolicy> {
    Box::new(AllOf(vec![
        Box::new(FixedCount(200)),
        Box::new(MemoryBudget(256 << 20)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `snapshots`, all but the last (the current version's) droppable,
    /// and `steps` bytes of steps.
    fn stats(snapshots: &[usize], steps: usize) -> HistoryStats {
        HistoryStats {
            versions: snapshots.len(),
            cursor: snapshots.len() - 1,
            droppable: snapshots.len() - 1,
            snapshot_bytes: snapshots.to_vec(),
            step_bytes: steps,
            blob_bytes: 0,
            undone_source_bytes: 0,
        }
    }

    /// Whether `policy` acts on `stats`: trims a snapshot or prunes.
    fn acts(policy: &dyn HistoryPolicy, stats: &HistoryStats) -> bool {
        policy.trim(stats) > 0 || policy.prunes(stats)
    }

    #[test]
    fn fixed_count() {
        assert_eq!(FixedCount(3).trim(&stats(&[1, 1, 1, 1, 1], 0)), 2);
        assert_eq!(FixedCount(10).trim(&stats(&[1, 1], 0)), 0);
        assert!(!FixedCount(3).prunes(&stats(&[1, 1, 1, 1, 1], 1 << 30)));
    }

    #[test]
    fn memory_budget_never_drops_what_must_stay() {
        assert_eq!(MemoryBudget(5).trim(&stats(&[10, 10, 10], 0)), 2);
        assert_eq!(MemoryBudget(25).trim(&stats(&[10, 10, 10], 0)), 1);
        let first_kept = HistoryStats {
            droppable: 1,
            ..stats(&[10, 10, 10], 0)
        };
        assert_eq!(MemoryBudget(5).trim(&first_kept), 1);
    }

    #[test]
    fn memory_budget_counts_steps_but_drops_only_snapshots() {
        assert_eq!(MemoryBudget(35).trim(&stats(&[10, 10, 10], 10)), 1);
        // Steps alone over the budget: every snapshot but the current one.
        assert_eq!(MemoryBudget(100).trim(&stats(&[10, 10, 10], 200)), 2);
    }

    #[test]
    fn memory_budget_counts_undone_sources_but_not_blobs() {
        let undone = |bytes| HistoryStats {
            blob_bytes: 1 << 30,
            undone_source_bytes: bytes,
            ..stats(&[10, 10, 10], 0)
        };
        assert_eq!(MemoryBudget(35).trim(&undone(0)), 0);
        assert_eq!(MemoryBudget(35).trim(&undone(10)), 1);
        assert!(!MemoryBudget(35).prunes(&undone(10)));
        // Undone sources alone over the budget: pruning frees them.
        assert!(MemoryBudget(100).prunes(&undone(200)));
    }

    /// A budget prunes only when trimming every snapshot it may wouldn't
    /// make history fit: the snapshots that must stay count, the others
    /// don't.
    #[test]
    fn memory_budget_prunes_what_trimming_cant_fit() {
        let at = stats(&[10, 10, 10], 85);
        assert_eq!(at.untrimmable_bytes(), 95);
        assert!(!MemoryBudget(95).prunes(&at));
        assert!(MemoryBudget(94).prunes(&at));
        assert!(MemoryBudget(100).trim(&at) > 0 && !MemoryBudget(100).prunes(&at));
    }

    /// A policy that can't act, as `could_act` says from the totals alone,
    /// trims nothing and prunes nothing.
    #[test]
    fn could_act_is_never_wrong_to_say_no() {
        let policies: [Box<dyn HistoryPolicy>; 3] = [
            Box::new(FixedCount(3)),
            Box::new(MemoryBudget(60)),
            Box::new(AllOf(vec![
                Box::new(FixedCount(3)),
                Box::new(MemoryBudget(60)),
            ])),
        ];
        for policy in &policies {
            for snapshots in 1..6 {
                for steps in [0, 20, 40, 60, 80] {
                    let at = stats(&vec![10; snapshots], steps);
                    if !policy.could_act(at.versions, at.total_bytes()) {
                        assert!(!acts(&**policy, &at), "{snapshots}, {steps}");
                    }
                }
            }
        }
    }

    #[test]
    fn all_of_takes_max() {
        let p = AllOf(vec![Box::new(FixedCount(2)), Box::new(MemoryBudget(1000))]);
        assert_eq!(p.trim(&stats(&[1, 1, 1, 1], 0)), 2);
        assert!(!p.prunes(&stats(&[1, 1, 1, 1], 0)));
        assert!(p.prunes(&stats(&[1, 1, 1, 1], 2000)));
    }
}
