//! A save, start to end (M10.5 §9.1): the version sealed, the plan, its
//! asks, the file written and checked, the record kept. [`save`] runs it
//! on one thread, for the command line and tests, until slice 19's
//! `Session::save`; the app runs [`write_file`] in the background between
//! [`Document::save_input`] and [`Document::record_save`], asking the
//! reader what the plan asks in between.

use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use in_progress::Progress;
use sheaf_util::Checksum;

use super::plan::{
    Ask, AskAnswers, Note, Onto, SavePlan, SaveStop, SaveTarget, Strategy, StrategyKind, Why,
    fallback, plan, unanswered,
};
use super::record::{SaveInput, SaveRecord};
use super::removal::Mode;
use crate::change::ChangeSet;
use crate::model::Document;
use crate::serialize::append::append_in_place;
use crate::serialize::durable::{self, FileStamp, WriteError};

/// What a save did, for the reader: where it wrote, how, and what they
/// should be told about the file.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveReport {
    pub path: Arc<Path>,
    /// How the file was written: None when it wasn't, the document being
    /// what the file holds already (an Unchanged plan).
    pub strategy: Option<StrategyKind>,
    /// Why it was rewritten, when it was (M10.5 §7.3).
    pub why: Vec<Why>,
    pub notes: Vec<Note>,
}

/// A file a save wrote, or found it needn't: its record, for the document
/// ([`record_in`](Self::record_in)), and what the reader should be told
/// about it, the plan's notes first.
pub struct Written {
    pub record: SaveRecord,
    pub notes: Vec<Note>,
    /// Whether the file was written: not for an Unchanged plan, whose
    /// record is the saved file's.
    pub wrote: bool,
    /// How it was written: the plan's strategy, or the Rewrite an Append
    /// was instead, for why it couldn't follow the file.
    pub strategy: Strategy,
}

impl Written {
    /// What the reader is told of it.
    pub fn report(&self) -> SaveReport {
        SaveReport {
            path: self.record.path.clone(),
            strategy: self.wrote.then_some(self.record.strategy),
            why: match &self.strategy {
                Strategy::Rewrite { why } if self.wrote => why.clone(),
                _ => Vec::new(),
            },
            notes: self.notes.clone(),
        }
    }

    /// Its version the saved one of `doc`, with its record
    /// ([`Document::record_save`]), the save noted when it wrote
    /// ([`Document::record_unchanged`] otherwise): the change, of nothing
    /// the document shows.
    pub fn record_in(self, doc: &mut Document) -> anyhow::Result<ChangeSet> {
        match self.wrote {
            true => doc.record_save(self.record),
            false => doc.record_unchanged(self.record),
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
    saved_as(doc, path, answers, Refusal::Rewrite, p, plan)
}

/// [`save`] by an Append, whatever the plan would decide: onto the
/// document's file, or a copy of it at another `path` (`Onto::CopyOf`),
/// as "Append anyway" saves (save policy's form transforms in the
/// section, the removal check's finds reported), asking nothing. One that
/// can't follow the file stops, rather than rewrite it. A document made in
/// memory appends onto its primary's bytes, written to `path` as the file
/// it was opened from ([`Document::opened_from`]). What the both-ways
/// tests' Append halves save with.
#[cfg(any(test, feature = "fixtures"))]
pub fn save_appended(doc: &mut Document, path: &Path) -> Result<SaveReport, SaveStop> {
    if doc.saved_record().is_none() {
        doc.opened_from(path).map_err(failed)?;
    }
    let append = |input: &SaveInput, target: &SaveTarget| {
        let onto = match target.same_file {
            true => Onto::Saved,
            false => Onto::CopyOf,
        };
        super::plan::forced_append(input, onto)
    };
    let answers = AskAnswers {
        append_anyway: true,
        stays_readable: true,
        found: true,
        ..Default::default()
    };
    saved_as(
        doc,
        path,
        &answers,
        Refusal::Stop,
        &in_progress::NoProgress,
        append,
    )
}

/// [`save`] by a Rewrite, whatever the plan would decide
/// ([`forced_rewrite`](super::plan::forced_rewrite)), giving up signatures
/// and an XFA part's values when it asks, but never a file another
/// program changed. What the both-ways tests' Rewrite halves save with.
#[cfg(any(test, feature = "fixtures"))]
pub fn save_rewritten(doc: &mut Document, path: &Path) -> Result<SaveReport, SaveStop> {
    let answers = AskAnswers {
        break_signatures: true,
        drop_xfa_values: true,
        ..Default::default()
    };
    saved_as(
        doc,
        path,
        &answers,
        Refusal::Rewrite,
        &in_progress::NoProgress,
        super::plan::forced_rewrite,
    )
}

/// The file an Append of `doc`'s current version onto its file writes, as
/// bytes, whatever the plan would decide, as "Append anyway" writes it
/// ([`save_appended`]): the saved file's bytes, then the section. Nothing
/// is written or recorded, and one that can't follow the file stops. A
/// document made in memory appends onto its primary, as though the file it
/// was opened from held those bytes ([`Document::append_input`]). What the
/// both-ways tests' Append halves write.
#[cfg(any(test, feature = "fixtures"))]
pub fn appended(doc: &Document) -> Result<Vec<u8>, SaveStop> {
    let input = doc.append_input(doc.current_version());
    let saved =
        (input.saved.as_ref()).ok_or_else(|| failed(anyhow::anyhow!("nothing is saved")))?;
    let base: Arc<[u8]> = match doc.saved_record() {
        Some(record) => std::fs::read(&record.path)
            .with_context(|| format!("reading {}", record.path.display()))
            .map_err(failed)?
            .into(),
        None => input.primary.bytes().clone(),
    };
    if base.len() as u64 != saved.record.stamp.len {
        return Err(SaveStop::Ask(Ask::FileChanged {
            path: saved.record.path.clone(),
        }));
    }
    let plan = super::plan::forced_append(&input, Onto::Saved);
    let out = super::append::append(
        &input,
        &plan,
        Arc::new(base.clone()),
        Mode::ReportAll,
        &in_progress::NoProgress,
    )
    .map_err(super::stopped)?;
    Ok([&base[..], &out.bytes].concat())
}

/// How a test that saves both ways writes: by a Rewrite, or by an Append
/// onto the document's file ([`both_ways!`](crate::both_ways)).
#[cfg(any(test, feature = "fixtures"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    Rewritten,
    Appended,
}

#[cfg(any(test, feature = "fixtures"))]
impl How {
    /// The file `doc`'s current version is written as this way, as bytes:
    /// nothing is written or recorded.
    pub fn write(self, doc: &Document) -> Vec<u8> {
        match self {
            How::Rewritten => crate::save::rewrite(&doc.snapshot()).unwrap(),
            How::Appended => appended(doc).unwrap(),
        }
    }

    /// `doc` saved to `path` this way, whatever the plan would decide: by
    /// a Rewrite ([`save_rewritten`]), or by an Append ([`save_appended`]).
    pub fn save(self, doc: &mut Document, path: &Path) -> SaveReport {
        match self {
            How::Rewritten => save_rewritten(doc, path).unwrap(),
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
/// and `$appended` by an Append. `$case` takes the [`How`]. Attributes
/// before `$name` are both's (an ignored tier's `#[ignore]`).
#[cfg(any(test, feature = "fixtures"))]
#[macro_export]
macro_rules! both_ways {
    ($(#[$m:meta])* $name:ident, $appended:ident: $case:path) => {
        $(#[$m])*
        #[test]
        fn $name() {
            $case($crate::materialize::How::Rewritten)
        }

        $(#[$m])*
        #[test]
        fn $appended() {
            $case($crate::materialize::How::Appended)
        }
    };
}

/// What a write does with an Append that can't follow the file
/// ([`append::Refused`](super::append::Refused)).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// It rewrites the file instead, asking first as a Rewrite does.
    Rewrite,
    /// It stops: a test's Append half, which must append.
    #[cfg(any(test, feature = "fixtures"))]
    Stop,
}

/// [`save`], its plan `planned`'s, an Append that can't follow the file
/// as `refusal` says.
fn saved_as(
    doc: &mut Document,
    path: &Path,
    answers: &AskAnswers,
    refusal: Refusal,
    p: &dyn Progress,
    planned: impl FnOnce(&SaveInput, &SaveTarget) -> SavePlan,
) -> Result<SaveReport, SaveStop> {
    let v = doc.current_version();
    let input = doc.save_input(v).map_err(failed)?;
    let target = doc.save_target(path);
    let plan = planned(&input, &target);
    match write(&input, &plan, target, answers, refusal, p) {
        Ok(written) => {
            let report = written.report();
            match written.record_in(doc) {
                Ok(_) => Ok(report),
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
/// `answers` answers every ask the plan has, as they leave it
/// ([`SavePlan::answered`]):
/// - **A Rewrite** replaces what's at the path durably (`durable::write`),
///   and when the path is the document's file, only while it's still the
///   file `target`'s stamp says, checked just before the rename, unless
///   the reader chose to overwrite it. Its checksum is taken as it's
///   written.
/// - **An Append** goes after the saved file in place, a restored
///   document's pending sections first (M10.5 §11.10), only while that's
///   still the file its record stamps (`append_in_place`), or (`CopyOf`)
///   after a copy of it at the path, checked against the record's checksum
///   when there's one (`durable::copy_appended`). Before a byte of it is
///   written, an Append that refers to objects the file hasn't got, or
///   that the removal check finds would leave removed text readable, or a
///   file that can't take one, is a Rewrite instead, its reason the
///   plan's (M10.5 §9.2), which asks before it breaks signatures that
///   hold. One that writes the catalog or the form's dictionary of a file
///   whose signatures hold, which none allows, asks as an Append they
///   don't allow does, unless the reader chose to append anyway. When
///   what they agreed to said something stays readable, the removal check
///   reports what it finds, but for a derived object whose removal the
///   save's path didn't say (a misclassification), which asks again,
///   saying the check found removed text, until they agree to that too;
///   else it finds removed text the plan didn't say, and asks again,
///   saying so.
/// - **An Unchanged plan** writes nothing: the record is the saved file's,
///   for the version saved.
///
/// A file another program changed stops the save with nothing written, to
/// ask [`Ask::FileChanged`], the version still sealed. The record's stamp
/// is the written file's.
pub fn write_file(
    input: &SaveInput,
    plan: &SavePlan,
    target: SaveTarget,
    answers: &AskAnswers,
    p: &dyn Progress,
) -> Result<Written, SaveStop> {
    write(input, plan, target, answers, Refusal::Rewrite, p)
}

/// [`write_file`], an Append that can't follow the file as `refusal` says.
fn write(
    input: &SaveInput,
    plan: &SavePlan,
    target: SaveTarget,
    answers: &AskAnswers,
    refusal: Refusal,
    p: &dyn Progress,
) -> Result<Written, SaveStop> {
    let plan = plan.answered(input, &target, answers);
    if let Some(ask) = unanswered(&plan, answers) {
        return Err(SaveStop::Ask(ask));
    }
    if plan.strategy == Strategy::Unchanged {
        let saved =
            (input.saved.as_ref()).ok_or_else(|| failed(anyhow::anyhow!("nothing is saved")))?;
        let record = SaveRecord {
            version: input.version,
            ..SaveRecord::clone(&saved.record)
        };
        return Ok(Written {
            record,
            notes: plan.notes,
            wrote: false,
            strategy: plan.strategy,
        });
    }
    let path = target.path.clone();
    let expect = (target.stamp).filter(|_| target.same_file && !answers.overwrite);
    // What the reader agreed would stay readable, the removal check
    // reports; after an Append anyway that said nothing would, it still
    // stops on what it finds, to ask again, and after one that listed what
    // the path removes, on a derived object's removal it didn't list,
    // until they agree to what the check finds.
    let mode = match answers {
        a if !a.append_anyway => Mode::Veto,
        a if !a.stays_readable => Mode::Anyway,
        a if !a.found => Mode::Report,
        _ => Mode::ReportAll,
    };
    let (plan, out) = match plan.strategy {
        Strategy::Append { .. } => {
            match super::append_onto_saved(input, &plan, &target, mode, p)? {
                Ok(out) => (plan, out),
                Err(refused) if refusal == Refusal::Rewrite => {
                    let plan = fallback(input, &target, &refused).answered(input, &target, answers);
                    if let Some(ask) = unanswered(&plan, answers) {
                        return Err(SaveStop::Ask(ask));
                    }
                    let out = super::materialize(input, &plan, target, p)?;
                    (plan, out)
                }
                Err(refused) => return Err(failed(refused)),
            }
        }
        _ => {
            let out = super::materialize(input, &plan, target, p)?;
            (plan, out)
        }
    };
    let changed = |e: WriteError| match e {
        WriteError::Changed { path } => SaveStop::Ask(Ask::FileChanged { path: path.into() }),
        e => failed(e),
    };
    let (stamp, content_hash) = match (&plan.strategy, &input.saved) {
        (Strategy::Append { onto: Onto::CopyOf }, Some(saved)) => {
            let saved = &saved.record;
            let sum = saved.content_hash.as_ref().map(Checksum::finish);
            let (stamp, copied) =
                durable::copy_appended(&saved.path, &saved.stamp, sum, &path, &out.bytes)
                    .map_err(changed)?;
            (stamp, Some(copied))
        }
        (Strategy::Append { .. }, Some(saved)) => {
            let saved = &saved.record;
            let stamp = match out.bytes.is_empty() {
                true => saved.stamp,
                false => append_in_place(&saved.path, &saved.stamp, &out.bytes).map_err(changed)?,
            };
            // The file's checksum, when it's known, goes on over the section.
            let sum = saved.content_hash.clone().map(|mut sum| {
                sum.write(&out.bytes);
                sum
            });
            (stamp, sum)
        }
        _ => {
            let mut sum = Checksum::new();
            sum.write(&out.bytes);
            durable::write(&path, &out.bytes, expect.as_ref()).map_err(changed)?;
            // The bytes written, with the file's own length and time: a
            // file another program put there since doesn't match it.
            let stamp = std::fs::metadata(&path)
                .and_then(|meta| FileStamp::of(&meta, &out.bytes))
                .with_context(|| format!("reading {}", path.display()))
                .map_err(failed)?;
            (stamp, Some(sum))
        }
    };
    let mut notes = plan.notes;
    notes.extend(out.notes);
    Ok(Written {
        record: out.record.stamped(stamp, content_hash),
        notes,
        wrote: true,
        strategy: plan.strategy,
    })
}

/// A save stopped by `e`, with its causes.
fn failed(e: impl Into<anyhow::Error>) -> SaveStop {
    SaveStop::Failed(format!("{:#}", e.into()))
}
