//! The planner (M10.5 slice 10, §9.2–9.3): what a save's path makes of
//! it, judged against the saved version. An undo back to the saved state
//! writes nothing; a comment and a fill append; what removes or replaces
//! text rewrites, and so does an Append the file hasn't the objects for; a
//! file another program changed asks. What a Rewrite says it's for.
//! Signatures by their impact: a certification's `/P`, an approval
//! signature, Reader's usage rights kept within what they allow, Save As
//! onto a copy, and "Append anyway". And what the write checks again: the
//! removal check under a misjudged path, and a section that writes what
//! the signatures don't allow.

use std::path::Path;
use std::sync::Arc;

use in_progress::NoProgress;
use lopdf::{Document as Pdf, Object, ObjectId, Stream, dictionary};
use sheaf_edit::{BookmarkPatch, Turn, Z};

use super::removal_tests::{layer, note, opened, said, scratch, text_file, warned};
use super::rewrite_tests::says;
use super::{
    AppendAnyway, Ask, AskAnswers, Note, Onto, SavePlan, SaveReport, SaveStop, Strategy,
    StrategyKind, Why, append, plan, save,
};
use crate::effects::{Flags, PathEffects, Removed, RemovedKind, SigClass, misjudged};
use crate::forms::fixture::{Form, row};
use crate::forms::sigs::{SigImpact, Verdict};
use crate::forms::{Finding, Value};
use crate::materialize::ObjKey;
use crate::model::{Document, FieldNode, Terminal};
use crate::prim::{Key, Val};
use crate::serialize::fixture::holds;
use crate::tx::fixture as fx;

/// What `doc`'s current version saved to `path` plans, nothing sealed.
fn planned(doc: &Document, path: &Path) -> SavePlan {
    plan(&doc.recovery_input(), &doc.save_target(path))
}

/// `doc` saved to `path` as the plan says, with `answers`.
fn saved(doc: &mut Document, path: &Path, answers: AskAnswers) -> Result<SaveReport, SaveStop> {
    save(doc, path, &answers, &NoProgress)
}

/// Append anyway, what the offer says stays readable too.
fn anyway() -> AskAnswers {
    AskAnswers {
        append_anyway: true,
        stays_readable: true,
        ..Default::default()
    }
}

fn rewrite() -> AskAnswers {
    AskAnswers {
        break_signatures: true,
        ..Default::default()
    }
}

/// What a save does to the one signature, of the field `sig`.
fn impact(verdict: Verdict) -> Vec<SigImpact> {
    vec![SigImpact {
        signature: "sig".into(),
        verdict,
    }]
}

/// A one-page form, its text field `t` empty, its signature field `sig`
/// signed as `sign` does, and Reader's usage rights for `rights`'s
/// `/Form` and `/Annots` when there are some.
fn signed(sign: impl FnOnce(&mut Form, ObjectId), rights: Option<(&[&str], &[&str])>) -> Vec<u8> {
    let mut f = Form::new(1);
    f.text("t", 0, row(0));
    let sig = f.signature("sig", 0, row(1));
    sign(&mut f, sig);
    if let Some((form, annots)) = rights {
        f.usage_rights_for(form, annots);
    }
    f.bytes()
}

/// Whether every signature of the file `bytes` fits it: its `/ByteRange`
/// ends where a revision does.
fn signatures_fit(bytes: &[u8]) -> bool {
    let findings = crate::forms::check(bytes, None).unwrap().findings;
    !(findings.iter()).any(|f| matches!(f, Finding::StaleSignature { .. }))
}

/// Whether the section after `before` in `now` writes an object at `id`'s
/// number: its header, `n g obj`, on a line of its own.
fn rewrites(before: &[u8], now: &[u8], id: ObjectId) -> bool {
    assert!(now.starts_with(before), "an Append keeps the file's bytes");
    let section = &now[before.len()..];
    let header = format!("{} {} obj", id.0, id.1);
    let at_line = |sep: &str| holds(section, format!("{sep}{header}").as_bytes());
    section.starts_with(header.as_bytes()) || at_line("\n") || at_line("\r")
}

/// The catalog of the file `bytes`, and its `/Perms`.
fn perms(bytes: &[u8]) -> (ObjectId, Object) {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let root = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let perms = pdf.catalog().unwrap().get(b"Perms").unwrap().clone();
    (root, perms)
}

/// A note added to page 0 of `doc`.
fn comment(doc: &mut Document, text: &str) {
    let page = doc.page(0).unwrap();
    let add = fx::add(page, &note(text));
    doc.commit_ops(vec![add], "note").unwrap();
}

/// A certification's `/P` permits after it what it says (§9.3): a
/// comment on a P3-certified file appends with nothing asked, its signed
/// bytes as they were. On a P2-certified one it asks first, offering the
/// Append anyway, which nothing would leave readable; answered Rewrite, it
/// rewrites ([`Why::Chosen`]).
#[test]
fn a_certification_permits_what_its_p_says() {
    let dir = scratch();
    for p in [3, 2] {
        let bytes = signed(|f, sig| f.certify(sig, p), None);
        let (path, mut doc) = opened(&dir, &format!("p{p}.pdf"), &bytes);
        comment(&mut doc, "a comment");
        let planned = planned(&doc, &path);
        assert_eq!(planned.strategy, Strategy::Append { onto: Onto::Saved });
        if p == 3 {
            assert_eq!(planned.asks, []);
            let report = saved(&mut doc, &path, AskAnswers::default()).unwrap();
            assert_eq!(report.strategy, Some(StrategyKind::Append));
            let now = std::fs::read(&path).unwrap();
            assert!(now.starts_with(&bytes) && signatures_fit(&now));
            continue;
        }
        let asks = Ask::Signatures {
            impact: impact(Verdict::Invalid),
            append_anyway: Some(AppendAnyway {
                stays_readable: Vec::new(),
            }),
            reason: None,
        };
        assert_eq!(planned.asks, std::slice::from_ref(&asks));
        let stop = saved(&mut doc, &path, AskAnswers::default());
        assert_eq!(stop, Err(SaveStop::Ask(asks)));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let report = saved(&mut doc, &path, rewrite()).unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
        assert_eq!(report.why, [Why::Chosen]);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A comment deleted from a signed file is a Rewrite for what it removes
/// (strict removal), which asks before it breaks the signature, offering
/// an Append anyway that leaves the comment readable in the signed
/// revision. Taken, it appends: the signed bytes are the file's first,
/// the signature fits them, and the reader is told what stays readable.
/// The removal check's find is the comment the offer listed: no
/// misclassification is logged or counted.
#[test]
fn a_comment_deleted_from_a_signed_file_offers_append_anyway() {
    let dir = scratch();
    let bytes = signed(|f, sig| f.sign(sig), None);
    let (path, mut doc) = opened(&dir, "signed.pdf", &bytes);
    // A comment, which an approval signature allows after it: appended.
    comment(&mut doc, "SECRETCOMMENT");
    let report = saved(&mut doc, &path, AskAnswers::default()).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    let before = std::fs::read(&path).unwrap();
    assert!(before.starts_with(&bytes) && signatures_fit(&before));
    let page = doc.page(0).unwrap();
    let id = doc.state().marks_on(page).next().unwrap().id;
    doc.commit_ops(vec![fx::delete_annots(&[id])], "delete")
        .unwrap();
    let removed = vec![Removed {
        kind: RemovedKind::Comments,
        count: 1,
    }];
    let why = Why::Removes(removed.clone());
    let planned = planned(&doc, &path);
    assert_eq!(
        planned.strategy,
        Strategy::Rewrite {
            why: vec![why.clone()]
        }
    );
    let asks = Ask::Signatures {
        impact: impact(Verdict::Invalid),
        append_anyway: Some(AppendAnyway {
            stays_readable: removed,
        }),
        reason: Some(why),
    };
    assert_eq!(planned.asks, std::slice::from_ref(&asks));
    assert_eq!(
        saved(&mut doc, &path, AskAnswers::default()),
        Err(SaveStop::Ask(asks))
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let counted = super::removal::vetoes_here();
    let (report, warnings) = warned(|| saved(&mut doc, &path, anyway()));
    let report = report.unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    let stays = super::Note(append::STAYS_READABLE.into());
    assert!(report.notes.contains(&stays), "{:?}", report.notes);
    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(super::removal::vetoes_here(), counted);
    let now = std::fs::read(&path).unwrap();
    assert!(now.starts_with(&before) && signatures_fit(&now));
    assert!(!doc.is_dirty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A page with a field inserted into a signed hybrid form changes the
/// form, so the save drops the XFA copy (§11.8), which with the page is
/// more than the signature allows after it: the Append asks, offering
/// itself anyway, the XFA copy what stays readable. Taken, the section's
/// form has no `/XFA`, and the signed bytes are as they were.
#[test]
fn a_page_inserted_into_a_signed_hybrid_form_offers_append_anyway() {
    let dir = scratch();
    let mut f = Form::new(1);
    let x = f.text("x[0]", 0, row(0));
    f.fill(x, "Ex");
    let sig = f.signature("sig", 0, row(1));
    f.sign(sig);
    f.xfa();
    let bytes = f.bytes();
    let (path, mut doc) = opened(&dir, "hybrid.pdf", &bytes);
    let mut other = Form::new(1);
    let z = other.text("z", 0, row(0));
    other.fill(z, "Zed");
    let kind = crate::store::BlobKind::file(Path::new("other.pdf"), None);
    let src = doc.store().open(other.bytes().into(), kind).unwrap();
    let insert = fx::insert(&src, [0], "other.pdf", None);
    doc.commit_ops(vec![insert], "insert").unwrap();
    let planned = planned(&doc, &path);
    assert_eq!(planned.strategy, Strategy::Append { onto: Onto::Saved });
    let xfa = Removed {
        kind: RemovedKind::XfaDatasets,
        count: 1,
    };
    let asks = Ask::Signatures {
        impact: impact(Verdict::Flagged),
        append_anyway: Some(AppendAnyway {
            stays_readable: vec![xfa],
        }),
        reason: None,
    };
    assert_eq!(planned.asks, [asks]);
    let report = saved(&mut doc, &path, anyway()).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    let now = std::fs::read(&path).unwrap();
    assert!(now.starts_with(&bytes) && signatures_fit(&now));
    let pdf = Pdf::load_mem(&now).unwrap();
    let form = pdf.catalog().unwrap().get(b"AcroForm").unwrap();
    let form = pdf.dereference(form).unwrap().1.as_dict().unwrap();
    assert!(!form.has(b"XFA"));
    let back = crate::open::file(&path, None).unwrap().doc;
    assert_eq!(back.state().terminals().count(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Field `name` of `doc` filled with `value`, as a fill no edit makes yet
/// (M10 slice 9) would.
fn fill(doc: &mut Document, name: &str, value: &str) {
    let st = doc.state();
    let (id, node) = (st.fields())
        .find(|(id, _)| st.full_name(*id).as_deref() == Some(name))
        .unwrap();
    let mut node = FieldNode::clone(node);
    let mut term = Terminal::clone(node.term.as_deref().unwrap());
    term.value = Some(Value::Text(value.into()));
    node.term = Some(Arc::new(term));
    let val = Some(Val::Field(Arc::new(node)));
    doc.put_for_test(Key::Field(id), val, "fill");
}

/// Reader's usage rights stay on an Append within their
/// `/TransformParams` (proposal 34), the signatures' mapping as
/// conservative as §9.3's: a fill of a P2-certified file whose rights
/// allow filling in, and a comment on a P3-certified one whose rights
/// allow creating comments, append with nothing asked, neither the
/// catalog nor the signature written again. A comment on a P2-certified
/// file whose rights allow it asks: P = 2 allows only filling in.
#[test]
fn usage_rights_stay_within_what_they_allow() {
    let dir = scratch();
    type Change = fn(&mut Document);
    let cases: [(i64, &[&str], &[&str], Change); 2] = [
        (2, &["FillIn"], &[], |d| fill(d, "t", "filled")),
        (3, &[], &["Create"], |d| comment(d, "a comment")),
    ];
    for (i, (p, form, annots, change)) in cases.into_iter().enumerate() {
        let bytes = signed(|f, sig| f.certify(sig, p), Some((form, annots)));
        let (path, mut doc) = opened(&dir, &format!("rights-{i}.pdf"), &bytes);
        let sig = doc
            .state()
            .terminals()
            .find(|(_, _, t)| t.value == Some(Value::Signed));
        let sig = sig.unwrap().1.src.as_ref().unwrap().id;
        let st = doc.state();
        let t = (st.terminals()).find(|(id, _, _)| st.full_name(*id).as_deref() == Some("t"));
        let t = t.unwrap().1.src.as_ref().unwrap().id;
        change(&mut doc);
        let planned = planned(&doc, &path);
        assert_eq!(planned.strategy, Strategy::Append { onto: Onto::Saved });
        assert_eq!((planned.asks, planned.transforms), (vec![], vec![]), "{i}");
        let report = saved(&mut doc, &path, AskAnswers::default()).unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Append));
        let now = std::fs::read(&path).unwrap();
        let (catalog, was) = perms(&bytes);
        assert!(!rewrites(&bytes, &now, catalog), "{i}: the catalog");
        assert!(!rewrites(&bytes, &now, sig), "{i}: the signature");
        // The fill writes its field, which shows the check sees one.
        assert_eq!(rewrites(&bytes, &now, t), i == 0, "{i}: the field");
        assert_eq!(perms(&now), (catalog, was), "{i}");
        assert!(signatures_fit(&now), "{i}");
    }
    let bytes = signed(|f, sig| f.certify(sig, 2), Some((&[], &["Create"])));
    let (path, mut doc) = opened(&dir, "p2-comment.pdf", &bytes);
    comment(&mut doc, "a comment");
    let planned = planned(&doc, &path);
    assert!(
        matches!(&planned.asks[..], [Ask::Signatures { impact, append_anyway: Some(_), .. }]
            if *impact == self::impact(Verdict::Invalid)),
        "{:?}",
        planned.asks
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Save As of a P1-certified file nothing changed plans an Append onto a
/// copy of it that asks nothing: it writes no section, so the copy is the
/// signed bytes, which the certification still covers.
#[test]
fn save_as_of_an_untouched_p1_file_copies_it_and_asks_nothing() {
    let dir = scratch();
    let bytes = signed(|f, sig| f.certify(sig, 1), None);
    let (_, mut doc) = opened(&dir, "p1.pdf", &bytes);
    let copy = dir.join("copy.pdf");
    let planned = planned(&doc, &copy);
    assert_eq!(planned.strategy, Strategy::Append { onto: Onto::CopyOf });
    assert_eq!(planned.asks, []);
    let report = saved(&mut doc, &copy, AskAnswers::default()).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    assert_eq!(std::fs::read(&copy).unwrap(), bytes);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Save As of a signed document plans an Append onto a copy of its file
/// (proposal 32): the copy is the signed bytes and a section, the
/// signature fits it, and the file it copied is untouched.
#[test]
fn save_as_of_a_signed_file_copies_and_appends() {
    let dir = scratch();
    let bytes = signed(|f, sig| f.sign(sig), None);
    let (path, mut doc) = opened(&dir, "signed.pdf", &bytes);
    comment(&mut doc, "a comment");
    let copy = dir.join("copy.pdf");
    let planned = planned(&doc, &copy);
    assert_eq!(planned.strategy, Strategy::Append { onto: Onto::CopyOf });
    assert_eq!(planned.asks, []);
    let report = saved(&mut doc, &copy, AskAnswers::default()).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    let copied = std::fs::read(&copy).unwrap();
    assert!(copied.len() > bytes.len() && copied.starts_with(&bytes));
    assert!(signatures_fit(&copied));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(&**doc.file().unwrap().0, copy.as_path());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A bookmark renamed after a save replaces its title (§7.1), which an
/// Append would leave readable: the save is a Rewrite, and the old title
/// is gone from the file, decoded or not.
#[test]
fn a_bookmark_renamed_rewrites_and_its_old_title_goes() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    let target = Some(doc.page(1).unwrap());
    let insert = fx::bookmark(None, None, "OLDTITLE", target);
    doc.commit_ops(vec![insert], "bookmark").unwrap();
    let report = saved(&mut doc, &path, AskAnswers::default()).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    assert!(says(&std::fs::read(&path).unwrap(), "OLDTITLE"));
    let id = doc.state().bookmarks_preorder()[0].1;
    let rename = fx::patch_bookmark(
        id,
        BookmarkPatch {
            title: Some("NEWTITLE".into()),
            ..Default::default()
        },
    );
    doc.commit_ops(vec![rename], "rename").unwrap();
    let report = saved(&mut doc, &path, AskAnswers::default()).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    let retitled = Removed {
        kind: RemovedKind::BookmarkTitles,
        count: 1,
    };
    assert_eq!(report.why, [Why::ReplacesText(vec![retitled])]);
    let now = std::fs::read(&path).unwrap();
    assert!(!says(&now, "OLDTITLE") && says(&now, "NEWTITLE"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Pages deleted and saved (a Rewrite, which prunes their objects), the
/// delete undone and saved again: the plan appends what comes back, the
/// section finds the file hasn't got the pages' objects, and the save is a
/// Rewrite for it, which reopens with every page.
#[test]
fn pages_deleted_saved_and_undone_come_back_by_a_rewrite() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let gone = doc.page(1).unwrap();
    doc.commit_ops(vec![fx::delete(&[gone])], "delete").unwrap();
    let report = saved(&mut doc, &path, AskAnswers::default()).unwrap();
    let page = Removed {
        kind: RemovedKind::Pages,
        count: 1,
    };
    assert_eq!(report.why, [Why::Removes(vec![page])]);
    assert_eq!(said(&path), ["page 0", "page 2"]);
    doc.undo().unwrap();
    let planned = planned(&doc, &path);
    assert_eq!(planned.strategy, Strategy::Append { onto: Onto::Saved });
    let report = saved(&mut doc, &path, AskAnswers::default()).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    assert_eq!(report.why, [Why::BaseObjectsMissing]);
    assert!(!doc.is_dirty());
    assert_eq!(said(&path), ["page 0", "page 1", "page 2"]);
    let back = crate::open::file(&path, None).unwrap().doc;
    assert_eq!(back.page_count(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Back at the saved state by another route (a turn and its turn back),
/// the plan is Unchanged, with a header that numbers the pages saved
/// before it or not: nothing is written, its bytes and time as they were,
/// and the document is clean.
#[test]
fn back_at_the_saved_state_nothing_is_written() {
    let dir = scratch();
    for header in [false, true] {
        let name = format!("a-{header}.pdf");
        let (path, mut doc) = opened(&dir, &name, &text_file(2));
        if header {
            let set = fx::set_header(fx::header("Head <<1>>"), None);
            doc.commit_ops(vec![set], "header").unwrap();
            let report = saved(&mut doc, &path, AskAnswers::default()).unwrap();
            assert_eq!(report.strategy, Some(StrategyKind::Append));
        }
        let before = std::fs::read(&path).unwrap();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let first = doc.page(0).unwrap();
        for by in [Turn::Quarter, Turn::Back] {
            doc.commit_ops(vec![fx::rotate(&[first], by)], "turn")
                .unwrap();
        }
        assert!(doc.is_dirty());
        assert_eq!(planned(&doc, &path).strategy, Strategy::Unchanged);
        let report = saved(&mut doc, &path, AskAnswers::default()).unwrap();
        assert_eq!(report.strategy, None, "{header}");
        assert!(!doc.is_dirty());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), mtime);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file another program changed since sheaf read it plans a Rewrite for
/// it, which asks first: nothing could append to bytes sheaf didn't write.
#[test]
fn a_file_changed_on_disk_plans_a_rewrite_that_asks() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    let first = doc.page(0).unwrap();
    doc.commit_ops(vec![fx::rotate(&[first], Turn::Quarter)], "turn")
        .unwrap();
    let mut theirs = std::fs::read(&path).unwrap();
    theirs.extend_from_slice(b"\n");
    std::fs::write(&path, &theirs).unwrap();
    let planned = planned(&doc, &path);
    assert_eq!(
        planned.strategy,
        Strategy::Rewrite {
            why: vec![Why::FileChanged]
        }
    );
    let asks = Ask::FileChanged {
        path: path.as_path().into(),
    };
    assert_eq!(planned.asks, std::slice::from_ref(&asks));
    let stop = saved(&mut doc, &path, AskAnswers::default());
    assert_eq!(stop, Err(SaveStop::Ask(asks)));
    assert_eq!(std::fs::read(&path).unwrap(), theirs);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A path judged as removing nothing.
fn removes_nothing(path: &mut PathEffects) {
    path.flags.remove(Flags::REMOVES | Flags::REPLACES_TEXT);
    path.removed.clear();
    path.removing.clear();
}

/// A deleted mark the planner misjudges as removing nothing: it plans an
/// Append, whose removal check finds the mark's text in what the section
/// unlinks, so the save is a Rewrite, which leaves it out, and the
/// misclassification is counted.
#[test]
fn a_misjudged_removal_is_vetoed_through_the_plan() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    comment(&mut doc, "a secret note");
    let report = saved(&mut doc, &path, AskAnswers::default()).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    assert!(holds(&std::fs::read(&path).unwrap(), b"a secret note"));
    let page = doc.page(0).unwrap();
    let id = doc.state().marks_on(page).next().unwrap().id;
    doc.commit_ops(vec![fx::delete_annots(&[id])], "delete")
        .unwrap();
    let before = super::removal::vetoes_here();
    let (report, warnings) = warned(|| {
        misjudged(removes_nothing, || {
            let planned = planned(&doc, &path);
            assert_eq!(planned.strategy, Strategy::Append { onto: Onto::Saved });
            saved(&mut doc, &path, AskAnswers::default()).unwrap()
        })
    });
    let key = ObjKey::Annot(id);
    assert_eq!(report.why, [Why::RemovalCheck { key }]);
    assert_eq!(super::removal::vetoes_here().derived, before.derived + 1);
    let logged = super::removal::RemovalMisclassified { key }.to_string();
    assert_eq!(warnings, [logged]);
    assert!(!holds(&std::fs::read(&path).unwrap(), b"a secret note"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// On a P2-certified file, whose signature allows no comment: a comment
/// appended anyway, then deleted and another added, the planner misjudging
/// the delete as removing nothing. The plan is an Append the signature
/// doesn't allow, offered anyway with nothing said to stay readable;
/// taken, the removal check still finds the deleted comment's text, so the
/// save asks again, saying the text stays readable, and writes nothing.
/// Taken again, it appends, and the reader is told.
#[test]
fn an_append_anyway_the_signatures_asked_still_stops_on_removed_text() {
    let dir = scratch();
    let bytes = signed(|f, sig| f.certify(sig, 2), None);
    let (path, mut doc) = opened(&dir, "p2.pdf", &bytes);
    comment(&mut doc, "a secret note");
    let signatures = AskAnswers {
        append_anyway: true,
        ..Default::default()
    };
    let report = saved(&mut doc, &path, signatures).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    let page = doc.page(0).unwrap();
    let id = doc.state().marks_on(page).next().unwrap().id;
    doc.commit_ops(vec![fx::delete_annots(&[id])], "delete")
        .unwrap();
    comment(&mut doc, "another note");
    misjudged(removes_nothing, || {
        let planned = planned(&doc, &path);
        assert_eq!(planned.strategy, Strategy::Append { onto: Onto::Saved });
        let offered = Ask::Signatures {
            impact: impact(Verdict::Invalid),
            append_anyway: Some(AppendAnyway {
                stays_readable: Vec::new(),
            }),
            reason: None,
        };
        assert_eq!(planned.asks, [offered]);
        let before = std::fs::read(&path).unwrap();
        let offer = AppendAnyway {
            stays_readable: vec![Removed {
                kind: RemovedKind::Found,
                count: 1,
            }],
        };
        let asks = Ask::Signatures {
            impact: impact(Verdict::Invalid),
            append_anyway: Some(offer.clone()),
            reason: Some(Why::RemovalCheck {
                key: ObjKey::Annot(id),
            }),
        };
        let stop = saved(&mut doc, &path, signatures);
        assert_eq!(stop, Err(SaveStop::Ask(asks)));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let mut answers = signatures;
        answers.take(&offer);
        let report = saved(&mut doc, &path, answers).unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Append));
        let told = Note(append::STAYS_READABLE.into());
        assert!(report.notes.contains(&told), "{:?}", report.notes);
    });
    let now = std::fs::read(&path).unwrap();
    assert!(now.starts_with(&bytes) && signatures_fit(&now));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A path judged as replacing no text: what it removes whole is still
/// said, but not a comment whose text it replaced.
fn replaces_nothing(path: &mut PathEffects) {
    path.flags.remove(Flags::REPLACES_TEXT);
    path.removed.retain(|r| !r.kind.is_text());
    path.removing.retain(|_, flags| {
        flags.remove(Flags::REPLACES_TEXT);
        !flags.is_empty()
    });
}

/// On a P2-certified file, whose signature allows no comment: two comments
/// appended anyway, then one deleted and the other's text replaced, the
/// planner misjudging the replaced text as none (§11.9). The plan is a
/// Rewrite for the comment removed, whose ask offers an Append anyway that
/// lists it alone; taken, the removal check finds the other's old text
/// too, which the offer didn't list: a misclassification, logged and
/// counted, so the save asks again, saying the text it found stays
/// readable, and writes nothing. Taken again, it appends, and the reader
/// is told; the misclassification is logged and counted again.
#[test]
fn an_append_anyway_that_listed_a_removal_still_stops_on_one_it_did_not() {
    let dir = scratch();
    let bytes = signed(|f, sig| f.certify(sig, 2), None);
    let (path, mut doc) = opened(&dir, "p2.pdf", &bytes);
    comment(&mut doc, "a secret note");
    comment(&mut doc, "another secret");
    let signatures = AskAnswers {
        append_anyway: true,
        ..Default::default()
    };
    let report = saved(&mut doc, &path, signatures).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    let page = doc.page(0).unwrap();
    let mark = |text: &str| {
        (doc.state().marks_on(page))
            .find(|a| a.contents == text)
            .unwrap()
            .clone()
    };
    let (gone, mut kept) = (mark("a secret note").id, mark("another secret"));
    doc.commit_ops(vec![fx::delete_annots(&[gone])], "delete")
        .unwrap();
    kept.contents = "a harmless note".into();
    doc.commit_ops(vec![fx::update(&kept)], "edit").unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(holds(&before, b"another secret"));
    let key = ObjKey::Annot(kept.id);
    let logged = super::removal::RemovalMisclassified { key }.to_string();
    let counted = super::removal::vetoes_here().derived;
    misjudged(replaces_nothing, || {
        let listed = vec![Removed {
            kind: RemovedKind::Comments,
            count: 1,
        }];
        let why = Why::Removes(listed.clone());
        let planned = planned(&doc, &path);
        assert_eq!(
            planned.strategy,
            Strategy::Rewrite {
                why: vec![why.clone()]
            }
        );
        let offer = AppendAnyway {
            stays_readable: listed.clone(),
        };
        let offered = Ask::Signatures {
            impact: impact(Verdict::Invalid),
            append_anyway: Some(offer.clone()),
            reason: Some(why),
        };
        assert_eq!(planned.asks, [offered]);
        let mut answers = AskAnswers::default();
        answers.take(&offer);
        let (stop, warnings) = warned(|| saved(&mut doc, &path, answers));
        let mut found = listed;
        found.push(Removed {
            kind: RemovedKind::Found,
            count: 1,
        });
        let offer = AppendAnyway {
            stays_readable: found,
        };
        let asks = Ask::Signatures {
            impact: impact(Verdict::Invalid),
            append_anyway: Some(offer.clone()),
            reason: Some(Why::RemovalCheck { key }),
        };
        assert_eq!(stop, Err(SaveStop::Ask(asks)));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(warnings, std::slice::from_ref(&logged));
        assert_eq!(super::removal::vetoes_here().derived, counted + 1);
        answers.take(&offer);
        let (report, warnings) = warned(|| saved(&mut doc, &path, answers));
        let report = report.unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Append));
        let told = Note(append::STAYS_READABLE.into());
        assert!(report.notes.contains(&told), "{:?}", report.notes);
        assert_eq!(warnings, [logged]);
        assert_eq!(super::removal::vetoes_here().derived, counted + 2);
    });
    let now = std::fs::read(&path).unwrap();
    assert!(now.starts_with(&before) && signatures_fit(&now));
    let _ = std::fs::remove_dir_all(&dir);
}

/// On a signed file, a page's recognized text appended anyway, then
/// recognized again: the plan is a Rewrite for the layer it replaces, its
/// ask offering an Append anyway that lists it. Taken, the removal check
/// finds the old layer's words gone from what's drawn over the page, which
/// the page's slot said (the recognized text, not a generator's header
/// and footer): reported, with nothing logged or counted (slice 10's
/// review, after the commit).
#[test]
fn a_page_recognized_again_on_a_signed_file_appends_anyway_as_listed() {
    let dir = scratch();
    let bytes = signed(|f, sig| f.sign(sig), None);
    let (path, mut doc) = opened(&dir, "signed.pdf", &bytes);
    let page = doc.page(0).unwrap();
    let ocr = |text: &str| fx::ocr([(page, layer(text))]);
    doc.commit_ops(ocr("firstwords"), "ocr").unwrap();
    let report = saved(&mut doc, &path, anyway()).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    doc.commit_ops(ocr("secondwords"), "ocr").unwrap();
    let planned = planned(&doc, &path);
    let removed = vec![Removed {
        kind: RemovedKind::OcrLayers,
        count: 1,
    }];
    let Strategy::Rewrite { why } = &planned.strategy else {
        panic!("{:?}", planned.strategy);
    };
    assert_eq!(why, &[Why::ReplacesText(removed.clone())]);
    let offered = planned.asks.iter().find_map(|a| match a {
        Ask::Signatures { append_anyway, .. } => append_anyway.clone(),
        _ => None,
    });
    let offer = AppendAnyway {
        stays_readable: removed,
    };
    assert_eq!(offered, Some(offer.clone()));
    let before = std::fs::read(&path).unwrap();
    let counted = super::removal::vetoes_here();
    let mut answers = AskAnswers::default();
    answers.take(&offer);
    let (report, warnings) = warned(|| saved(&mut doc, &path, answers));
    let report = report.unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    let stays = Note(append::STAYS_READABLE.into());
    assert!(report.notes.contains(&stays), "{:?}", report.notes);
    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(super::removal::vetoes_here(), counted);
    let now = std::fs::read(&path).unwrap();
    assert!(now.starts_with(&before) && signatures_fit(&now));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A path judged as comments alone.
fn comments_alone(path: &mut PathEffects) {
    path.sig = SigClass::ANNOT;
}

/// Page labels added to a P3-certified file, misjudged as a comment, which
/// the certification allows: the plan appends, asking nothing, but the
/// section writes the catalog, which no signature allows after it, so the
/// write stops before a byte is written, asking as the plan would have
/// (it fails closed). Answered Rewrite, it rewrites.
#[test]
fn a_section_writing_the_catalog_unplanned_asks() {
    let dir = scratch();
    let bytes = signed(|f, sig| f.certify(sig, 3), None);
    let (path, mut doc) = opened(&dir, "p3.pdf", &bytes);
    let labels = fx::labels(&[crate::labels::LabelSpec {
        start: doc.page(0).unwrap(),
        style: crate::labels::Style::LowerRoman,
        prefix: String::new(),
        first: 1,
    }]);
    doc.commit_ops(vec![labels], "labels").unwrap();
    misjudged(comments_alone, || {
        let planned = planned(&doc, &path);
        assert_eq!(planned.strategy, Strategy::Append { onto: Onto::Saved });
        assert_eq!(planned.asks, []);
        let asks = Ask::Signatures {
            impact: impact(Verdict::Invalid),
            append_anyway: Some(AppendAnyway {
                stays_readable: Vec::new(),
            }),
            reason: None,
        };
        let v = doc.current_version();
        let stop = saved(&mut doc, &path, AskAnswers::default());
        assert_eq!(stop, Err(SaveStop::Ask(asks)));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(doc.is_dirty() && doc.sealed(v) == 0);
        let report = saved(&mut doc, &path, rewrite()).unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
        assert_eq!(report.why, [Why::Chosen]);
    });
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the reader is told a Rewrite is for (§7.3): what it removes or
/// replaces, counted, why an Append couldn't be written, or that a
/// recovered document's sections removed text.
#[test]
fn a_rewrite_says_why() {
    let removed = |kind, count| Removed { kind, count };
    let gone = Why::Removes(vec![
        removed(RemovedKind::Pages, 1),
        removed(RemovedKind::Comments, 2),
        removed(RemovedKind::Bookmarks, 1),
    ]);
    assert_eq!(
        gone.to_string(),
        "1 page, 2 comments and 1 bookmark removed since the last save"
    );
    let replaced = Why::ReplacesText(vec![removed(RemovedKind::CommentText, 2)]);
    assert_eq!(
        replaced.to_string(),
        "2 comments' text changed since the last save"
    );
    assert_eq!(
        Why::BaseObjectsMissing.to_string(),
        "the file no longer holds all the changes need"
    );
    // A recovery sidecar's flags say its sections removed text, not what.
    assert_eq!(
        Why::RecoveredRemovals.to_string(),
        "the recovered changes removed text the file holds"
    );
    assert_eq!(
        removed(RemovedKind::Recovered, 1).to_string(),
        "what the recovered changes removed"
    );
}

/// A file of one page whose XMP metadata holds `description`, its claims
/// ([`crate::source::tests::xmp`]).
fn claiming(description: &str) -> Vec<u8> {
    let mut pdf = Pdf::load_mem(&text_file(1)).unwrap();
    let xml = crate::source::tests::xmp(description);
    let metadata = Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        xml.into_bytes(),
    );
    let metadata = pdf.add_object(metadata);
    pdf.catalog_mut().unwrap().set("Metadata", metadata);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// A save that draws a watermark, a background or the header and footer
/// on a file claiming PDF/A or PDF/UA says the saved file no longer
/// conforms, naming the claims and what's drawn (proposal 39): a Rewrite
/// all it draws, an Append those it changes. One that draws nothing
/// doesn't, nor one of a file that claims nothing, and a claim that can't
/// be read is logged.
#[test]
fn a_claim_what_sheaf_draws_breaks_gets_a_note() {
    let dir = scratch();
    let pdfa = "<rdf:Description rdf:about=\"\" \
         xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\" pdfaid:part=\"2\" pdfaid:conformance=\"B\"/>";
    let pdfua = "<rdf:Description rdf:about=\"\" \
         xmlns:pdfuaid=\"http://www.aiim.org/pdfua/ns/id/\" pdfuaid:part=\"1\"/>";
    let unreadable = "<rdf:Description xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\" \
         pdfaid:part=\"two\"/>";
    let claims = |plan: &SavePlan| -> Vec<String> {
        (plan.notes.iter())
            .map(|n| n.0.clone())
            .filter(|n| n.contains("claims"))
            .collect()
    };
    let watermark = || fx::set_watermark(fx::watermark("Draft"), Z::Over, None);
    let told = |claimed: &str| {
        format!(
            "This file claims {claimed}; the watermark sheaf draws on it isn't made to conform, \
             so the saved file no longer does"
        )
    };
    // PDF/A-2b, before a save and after one, when an Append draws it.
    let (path, mut doc) = opened(&dir, "pdfa.pdf", &claiming(pdfa));
    assert_eq!(claims(&planned(&doc, &path)), Vec::<String>::new());
    doc.commit_ops(vec![watermark()], "watermark").unwrap();
    let plan = planned(&doc, &path);
    assert!(matches!(plan.strategy, Strategy::Append { .. }), "{plan:?}");
    assert_eq!(claims(&plan), [told("PDF/A-2b")]);
    let report = saved(&mut doc, &path, AskAnswers::default()).unwrap();
    assert!(report.notes.contains(&Note(told("PDF/A-2b"))), "{report:?}");
    // An Append that draws none, the claim broken already.
    comment(&mut doc, "later");
    let plan = planned(&doc, &path);
    assert!(matches!(plan.strategy, Strategy::Append { .. }), "{plan:?}");
    assert_eq!(claims(&plan), Vec::<String>::new());
    // A Rewrite, for the watermark's text replaced, says it of everything
    // it draws.
    let page = doc.page(0).unwrap();
    let replaced = fx::set_watermark(fx::watermark("Final"), Z::Over, None);
    let background = fx::set_watermark(fx::watermark("Copy"), Z::Under, None);
    let header = fx::set_header(fx::header("Header"), Some(&[page]));
    doc.commit_ops(vec![replaced, background, header], "more")
        .unwrap();
    let plan = planned(&doc, &path);
    assert!(
        matches!(plan.strategy, Strategy::Rewrite { .. }),
        "{plan:?}"
    );
    let all = "This file claims PDF/A-2b; the watermark, the background and the header and \
         footer sheaf draws on it aren't made to conform, so the saved file no longer does";
    assert_eq!(claims(&plan), [all]);
    // PDF/UA-1, and both.
    let (path, mut doc) = opened(&dir, "pdfua.pdf", &claiming(pdfua));
    doc.commit_ops(vec![watermark()], "watermark").unwrap();
    assert_eq!(claims(&planned(&doc, &path)), [told("PDF/UA-1")]);
    let (path, mut doc) = opened(&dir, "both.pdf", &claiming(&format!("{pdfa}{pdfua}")));
    doc.commit_ops(vec![watermark()], "watermark").unwrap();
    let both = told("PDF/A-2b and PDF/UA-1");
    assert_eq!(claims(&planned(&doc, &path)), [both]);
    // No claim, no note.
    let (path, mut doc) = opened(&dir, "plain.pdf", &text_file(1));
    doc.commit_ops(vec![watermark()], "watermark").unwrap();
    assert_eq!(claims(&planned(&doc, &path)), Vec::<String>::new());
    // A claim that can't be read: logged, no note.
    let (path, mut doc) = opened(&dir, "unreadable.pdf", &claiming(unreadable));
    doc.commit_ops(vec![watermark()], "watermark").unwrap();
    let (plan, logged) = warned(|| planned(&doc, &path));
    assert_eq!(claims(&plan), Vec::<String>::new());
    assert!(
        logged.iter().any(|w| w.contains("conformance claims")),
        "{logged:?}"
    );
}
