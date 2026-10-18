//! A save, start to end (M10.5 §9.1): the version sealed, the plan, its
//! asks, the file written and checked, the record kept. [`save`] runs it
//! on one thread, for the command line and tests, until slice 19's
//! `Session::save`; the app runs [`write_file`] in the background between
//! [`Document::save_input`] and [`Document::record_save`], asking the
//! reader what the plan asks in between.

use std::path::Path;
use std::sync::Arc;

use in_progress::Progress;

use super::plan::{
    Ask, AskAnswers, Note, SavePlan, SaveStop, SaveTarget, StrategyKind, plan, unanswered,
};
use super::record::{SaveInput, SaveRecord};
use crate::model::Document;
use crate::serialize::durable::{self, FileStamp, WriteError};

/// What a save did, for the reader: where it wrote, how, and what they
/// should be told about the file.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveReport {
    pub path: Arc<Path>,
    pub strategy: StrategyKind,
    pub notes: Vec<Note>,
}

/// A file a save wrote: its record, for [`Document::record_save`], and
/// what the reader should be told about it, the plan's notes first.
pub struct Written {
    pub record: SaveRecord,
    pub notes: Vec<Note>,
}

impl Written {
    /// What the reader is told of it.
    pub fn report(&self) -> SaveReport {
        SaveReport {
            path: self.record.path.clone(),
            strategy: self.record.strategy,
            notes: self.notes.clone(),
        }
    }
}

/// Save `doc`'s current version to `path`, the reader's answers to what
/// the plan asks given as `answers`: sealed while it's written, its file
/// checked as any output is, then the saved version, with the file's
/// record. Nothing in the document changes but which version is saved:
/// no version, state or source (M10.5 §9.1).
///
/// An ask `answers` doesn't answer stops it before anything is written
/// ([`SaveStop::Ask`]), as does a file another program changed, found
/// when it's planned or just before the rename. Whatever stops it leaves
/// the version unsealed ([`Document::abandon_save`]).
pub fn save(
    doc: &mut Document,
    path: &Path,
    answers: &AskAnswers,
    p: &dyn Progress,
) -> Result<SaveReport, SaveStop> {
    saved_as(doc, path, answers, p, plan)
}

/// [`save`] by an Append onto the document's file, whatever the plan would
/// decide: what the both-ways tests' Append halves save with. Slice 10
/// brings Append back; until then it stops ([`SaveStop::Failed`]), and
/// those halves are ignored.
#[cfg(any(test, feature = "fixtures"))]
pub fn save_appended(doc: &mut Document, path: &Path) -> Result<SaveReport, SaveStop> {
    use super::plan::{Onto, Strategy};
    let append = |_: &SaveInput, _: &SaveTarget| SavePlan {
        strategy: Strategy::Append { onto: Onto::Saved },
        asks: Vec::new(),
        transforms: Vec::new(),
        notes: Vec::new(),
    };
    let answers = AskAnswers::default();
    saved_as(doc, path, &answers, &in_progress::NoProgress, append)
}

/// The file an Append of `snap` onto its document's file writes, as bytes,
/// whatever the plan would decide: what the both-ways tests' Append halves
/// write. Until slice 10 it stops ([`SaveStop::Failed`]), and those halves
/// are ignored.
#[cfg(any(test, feature = "fixtures"))]
pub fn appended(snap: &crate::Snapshot) -> Result<Vec<u8>, SaveStop> {
    use super::plan::{Onto, Profile, Strategy};
    let input = SaveInput::detached(snap);
    let plan = SavePlan {
        strategy: Strategy::Append { onto: Onto::Saved },
        asks: Vec::new(),
        transforms: Vec::new(),
        notes: Vec::new(),
    };
    let target = SaveTarget {
        path: snap.file.clone().unwrap_or_else(|| Path::new("").into()),
        same_file: true,
        profile: Profile::default(),
        stamp: None,
    };
    let out = super::materialize(&input, &plan, target, &in_progress::NoProgress)?;
    Ok(out.bytes)
}

/// How a test that saves both ways writes: by a Rewrite, or by an Append
/// onto the document's file, which slice 10 brings back. Its Append half
/// is ignored until then ([`both_ways!`](crate::both_ways)).
#[cfg(any(test, feature = "fixtures"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    Rewritten,
    Appended,
}

#[cfg(any(test, feature = "fixtures"))]
impl How {
    /// The file `snap` is written as this way, as bytes.
    pub fn write(self, snap: &crate::Snapshot) -> Vec<u8> {
        match self {
            How::Rewritten => crate::save::rewrite(snap).unwrap(),
            How::Appended => appended(snap).unwrap(),
        }
    }

    /// `doc` saved to `path` this way: a Rewrite as [`save`] plans it, a
    /// signature or an XFA part's values given up when it asks, but never
    /// a file another program changed; or an Append ([`save_appended`]).
    pub fn save(self, doc: &mut Document, path: &Path) -> SaveReport {
        match self {
            How::Rewritten => {
                let answers = AskAnswers {
                    overwrite: false,
                    break_signatures: true,
                    drop_xfa_values: true,
                };
                save(doc, path, &answers, &in_progress::NoProgress).unwrap()
            }
            How::Appended => save_appended(doc, path).unwrap(),
        }
    }

    /// What a save this way is noted as.
    pub fn strategy(self) -> StrategyKind {
        match self {
            How::Rewritten => StrategyKind::Rewrite,
            How::Appended => StrategyKind::Append,
        }
    }
}

/// A test that saves both ways, twice: `$name` runs `$case` by a Rewrite
/// and `$appended` by an Append, which is ignored until slice 10 brings
/// Append back. `$case` takes the [`How`]. Attributes before `$name` are
/// its own (an ignored tier's `#[ignore]`).
#[cfg(any(test, feature = "fixtures"))]
#[macro_export]
macro_rules! both_ways {
    ($(#[$m:meta])* $name:ident, $appended:ident: $case:path) => {
        $(#[$m])*
        #[test]
        fn $name() {
            $case($crate::materialize::How::Rewritten)
        }

        #[test]
        #[ignore = "slice 10"]
        fn $appended() {
            $case($crate::materialize::How::Appended)
        }
    };
}

/// [`save`], its plan `planned`'s.
fn saved_as(
    doc: &mut Document,
    path: &Path,
    answers: &AskAnswers,
    p: &dyn Progress,
    planned: impl FnOnce(&SaveInput, &SaveTarget) -> SavePlan,
) -> Result<SaveReport, SaveStop> {
    let v = doc.current_version();
    let input = doc.save_input(v).map_err(failed)?;
    let target = doc.save_target(path);
    let plan = planned(&input, &target);
    match write_file(&input, &plan, target, answers, p) {
        Ok(written) => {
            let report = written.report();
            match doc.record_save(written.record) {
                Ok(()) => Ok(report),
                Err(e) => {
                    doc.abandon_save(v);
                    Err(failed(e))
                }
            }
        }
        Err(stop) => {
            doc.abandon_save(v);
            Err(stop)
        }
    }
}

/// `plan`'s file of `input`, written to `target` and its record made, once
/// `answers` answers every ask the plan has. The file replaces what's at
/// the path durably (`durable::write`), and when the path is the
/// document's file, only while it's still the file `target`'s stamp says,
/// checked just before the rename, unless the reader chose to overwrite
/// it: otherwise nothing is written, and the save stops to ask
/// [`Ask::FileChanged`]. The record's stamp is the written file's.
pub fn write_file(
    input: &SaveInput,
    plan: &SavePlan,
    target: SaveTarget,
    answers: &AskAnswers,
    p: &dyn Progress,
) -> Result<Written, SaveStop> {
    if let Some(ask) = unanswered(plan, answers) {
        return Err(SaveStop::Ask(ask));
    }
    let path = target.path.clone();
    let expect = (target.stamp).filter(|_| target.same_file && !answers.overwrite);
    let out = super::materialize(input, plan, target, p)?;
    match durable::write(&path, &out.bytes, expect.as_ref()) {
        Ok(()) => {}
        Err(WriteError::Changed { .. }) => return Err(SaveStop::Ask(Ask::FileChanged { path })),
        Err(e) => return Err(failed(e)),
    }
    // The bytes written, with the file's own length and time: a file
    // another program put there since doesn't match it.
    let stamp = std::fs::metadata(&path)
        .and_then(|meta| FileStamp::of(&meta, &out.bytes))
        .map_err(|e| {
            failed(anyhow::Error::new(e).context(format!("reading {}", path.display())))
        })?;
    let mut notes = plan.notes.clone();
    notes.extend(out.notes);
    Ok(Written {
        record: out.record.stamped(stamp),
        notes,
    })
}

/// A save stopped by `e`, with its causes.
fn failed(e: impl Into<anyhow::Error>) -> SaveStop {
    SaveStop::Failed(format!("{:#}", e.into()))
}
