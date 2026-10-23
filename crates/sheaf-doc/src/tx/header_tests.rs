//! The header and footer as `SetHeaderFooter` and `RemoveHeaderFooter` set
//! it (M10.5 slice 17): one generator, over the content, which a later one
//! replaces, keeping its id, date and file; its Bates numbers counted when
//! it's set and fixed then, by page, so a move never renumbers them and a
//! page added later has none; on every page, those added later too, or on
//! some; taken off pages by narrowing its scope, which reads the order
//! when it was on every page, with others' headers and footers there, a
//! page at a time, counted as a dialog counts them before.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use in_progress::{Level, NoProgress, Progress, Unit};
use sheaf_edit::{
    BatesSpec, HeaderFooterSpec, LabelStyle, Op, OutlinePolicy, PageNumbering, PageSel, ScopeSel,
    Transform, Z,
};

use super::fixture as fx;
use super::stale_tests::{on, planned};
use super::tests::{commit, doc_of};
use super::*;
use crate::content::PaginationKind;
use crate::model::{Document, Frozen, Generator, Scope};
use crate::prim::{Key, Val};

/// A footer of Bates numbers from 100, bottom right.
fn bates() -> HeaderFooterSpec {
    let mut spec = fx::header("");
    spec.texts[5] = "<<bates>>".into();
    spec.bates = Some(BatesSpec {
        start: 100,
        ..BatesSpec::default()
    });
    spec
}

/// The header and footer's generator.
fn drawn(d: &Document) -> Arc<Generator> {
    d.state()
        .header_footer()
        .expect("a header and footer")
        .clone()
}

/// The Bates numbers of `pages`.
fn numbers(d: &Document, pages: &[PageId]) -> Vec<Option<u64>> {
    let g = drawn(d);
    pages.iter().map(|&p| g.frozen.bates_number(p)).collect()
}

/// Bates numbers count the pages it covers in order from the start, when
/// it's set, and are fixed then: a move keeps each page's, by its id. It's
/// over the content, and replaces others' headers and footers.
#[test]
fn bates_numbers_are_fixed_when_it_is_set() {
    let mut d = doc_of(4, &[]);
    let all = [0, 1, 2, 3].map(|i| d.page(i).unwrap());
    let [p0, p1, p2, p3] = all;
    let set = fx::set_header(bates(), Some(&[p3, p1, p2]));
    let c = commit(&mut d, Actor::User, vec![set]).unwrap();
    let g = drawn(&d);
    assert_eq!(*c.effects.keys, BTreeSet::from([Key::Generator(g.id)]));
    let replaced = BTreeSet::from([PaginationKind::Header, PaginationKind::Footer]);
    assert_eq!((g.z, &g.replaces), (Z::Over, &replaced));
    assert_eq!(c.lines[0].text, "Set the header and footer");
    assert_eq!(numbers(&d, &all), [None, Some(100), Some(101), Some(102)]);
    let moved = fx::move_pages(&[p3, p2], Some(p0));
    commit(&mut d, Actor::User, vec![moved]).unwrap();
    assert_eq!(numbers(&d, &all), [None, Some(100), Some(101), Some(102)]);
    assert_eq!(d.state().check_invariants(), Ok(()));
}

/// Set again as it is, the order as it was, it changes nothing: its id,
/// date, file and numbers are the ones it has. Set again after a move, it
/// keeps its id, date and file, and numbers the pages in their new order.
#[test]
fn set_again_as_it_is_it_changes_nothing() {
    let mut d = doc_of(3, &[]);
    let pages: Vec<PageId> = d.state().order().iter().copied().collect();
    commit(&mut d, Actor::User, vec![fx::set_header(bates(), None)]).unwrap();
    let was = drawn(&d);
    assert_eq!(&*was.frozen.file, "a.pdf");
    let n = d.version_count();
    let c = commit(&mut d, Actor::User, vec![fx::set_header(bates(), None)]).unwrap();
    assert_eq!((c.version, d.version_count()), (None, n));
    assert_eq!(drawn(&d), was);
    // Saved as another file since, and moved: renumbered, as it was named.
    let stamp = crate::serialize::durable::FileStamp {
        len: 0,
        mtime: std::time::SystemTime::UNIX_EPOCH,
        tail: 0,
        window: 0,
    };
    let path = std::path::Path::new("saved").join("b.pdf");
    let v = d.current_version();
    let saved = crate::materialize::SaveRecord::opened(v, d.primary(), path.into(), stamp);
    d.record_save(saved).unwrap();
    let moved = fx::move_pages(&[pages[2]], Some(pages[0]));
    commit(&mut d, Actor::User, vec![moved]).unwrap();
    commit(&mut d, Actor::User, vec![fx::set_header(bates(), None)]).unwrap();
    let now = drawn(&d);
    assert_eq!(now.id, was.id);
    assert_eq!(
        (&now.frozen.date, &now.frozen.file),
        (&was.frozen.date, &was.frozen.file)
    );
    assert_eq!(numbers(&d, &pages), [Some(101), Some(102), Some(100)]);
    // With no text, there's none.
    commit(
        &mut d,
        Actor::User,
        vec![fx::set_header(fx::header(" "), None)],
    )
    .unwrap();
    assert!(d.state().generators().is_empty());
}

/// On every page, it's on those added later too, which have no Bates
/// number; on some, a duplicate's copy joins them, unnumbered too.
#[test]
fn a_page_added_later_is_covered_and_unnumbered() {
    let mut d = doc_of(2, &[]);
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    commit(&mut d, Actor::User, vec![fx::set_header(bates(), None)]).unwrap();
    let other = crate::store::blank(d.store(), 1);
    let inserted = fx::insert(&other, [0], "b.pdf", None);
    let c = commit(&mut d, Actor::User, vec![inserted]).unwrap();
    let new = c.created.pages().next().unwrap();
    let c = commit(&mut d, Actor::User, vec![fx::duplicate(&[p0])]).unwrap();
    let copy = c.created.pages().next().unwrap();
    let g = drawn(&d);
    assert!(g.covers(new) && g.covers(copy));
    let numbered = numbers(&d, &[p0, p1, new, copy]);
    assert_eq!(numbered, [Some(100), Some(101), None, None]);

    commit(
        &mut d,
        Actor::User,
        vec![fx::set_header(bates(), Some(&[p1]))],
    )
    .unwrap();
    let c = commit(&mut d, Actor::User, vec![fx::duplicate(&[p1])]).unwrap();
    let copy = c.created.pages().next().unwrap();
    let g = drawn(&d);
    assert_eq!(g.scope, Scope::Pages([p1, copy].into_iter().collect()));
    assert_eq!(numbers(&d, &[p1, copy]), [Some(100), None]);
    assert_eq!(d.state().check_invariants(), Ok(()));
}

/// A deleted page takes its Bates number with it, and no page is given it
/// again: the others keep theirs, and one inserted has none. The cascade
/// says the page it numbered went, though it's on every page.
#[test]
fn a_deleted_page_s_number_is_never_given_again() {
    let mut d = doc_of(3, &[]);
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    commit(&mut d, Actor::User, vec![fx::set_header(bates(), None)]).unwrap();
    let c = commit(&mut d, Actor::User, vec![fx::delete(&[p1])]).unwrap();
    let dropped = Cascade {
        rule: Rule::PageGenerators,
        objects: 1,
    };
    assert_eq!(c.cascades, [dropped]);
    let g = drawn(&d);
    assert_eq!(g.scope, Scope::All);
    assert_eq!(g.frozen.bates.as_ref().map(|b| b.len()), Some(2));
    assert_eq!(numbers(&d, &[p0, p2]), [Some(100), Some(102)]);
    let other = crate::store::blank(d.store(), 1);
    let inserted = fx::insert(&other, [0], "b.pdf", Some(p2));
    let c = commit(&mut d, Actor::User, vec![inserted]).unwrap();
    let new = c.created.pages().next().unwrap();
    assert_eq!(numbers(&d, &[p0, new, p2]), [Some(100), None, Some(102)]);
    d.undo().unwrap();
    d.undo().unwrap();
    assert_eq!(
        numbers(&d, &[p0, p1, p2]),
        [Some(100), Some(101), Some(102)]
    );
}

/// Bates numbers that would run past the largest there is are refused,
/// and those that end on it are given.
#[test]
fn bates_numbers_never_run_past_the_largest() {
    let mut d = doc_of(2, &[]);
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let from = |start| {
        let mut spec = bates();
        spec.bates.as_mut().unwrap().start = start;
        fx::set_header(spec, None)
    };
    let e = commit(&mut d, Actor::User, vec![from(u64::MAX)]).unwrap_err();
    assert!(matches!(e, EditError::Invalid { .. }), "{e}");
    assert!(d.state().header_footer().is_none());
    commit(&mut d, Actor::User, vec![from(u64::MAX - 1)]).unwrap();
    assert_eq!(numbers(&d, &[p0, p1]), [Some(u64::MAX - 1), Some(u64::MAX)]);
}

/// What an export keeps of `d` when it keeps `pages` alone.
fn kept(d: &Document, pages: &[PageId]) -> DocState {
    let mut xtx = ExportTx::new(&d.snapshot(), d.lower_cx(Actor::User));
    let keep = Transform::KeepPages {
        pages: PageSel::ids(pages.iter().copied()),
        outline: OutlinePolicy::Drop,
        labels: false,
        numbering: PageNumbering::AsKept,
    };
    xtx.apply(keep, &NoProgress).unwrap();
    xtx.state().clone()
}

/// An extract keeps the numbers of the pages it keeps, and the header and
/// footer only on those of its pages it keeps; it has none when it keeps
/// none of them.
#[test]
fn an_extract_keeps_the_numbers_of_the_pages_it_keeps() {
    let mut d = doc_of(4, &[]);
    let [p0, p1, p2, p3] = [0, 1, 2, 3].map(|i| d.page(i).unwrap());
    commit(&mut d, Actor::User, vec![fx::set_header(bates(), None)]).unwrap();
    let kept_of = kept(&d, &[p1, p3]);
    let g = kept_of.header_footer().unwrap();
    assert_eq!(g.scope, Scope::All);
    let want: im::OrdMap<PageId, u64> = [(p1, 101u64), (p3, 103)].into_iter().collect();
    assert_eq!(g.frozen.bates.as_deref(), Some(&want));

    commit(
        &mut d,
        Actor::User,
        vec![fx::set_header(bates(), Some(&[p0, p1]))],
    )
    .unwrap();
    let kept_of = kept(&d, &[p1, p3]);
    let g = kept_of.header_footer().unwrap();
    assert_eq!(g.scope, Scope::Pages(im::OrdSet::unit(p1)));
    let want: im::OrdMap<PageId, u64> = [(p1, 101u64)].into_iter().collect();
    assert_eq!(g.frozen.bates.as_deref(), Some(&want));
    assert!(kept(&d, &[p2, p3]).header_footer().is_none());
}

/// Taken off pages, it's narrowed to the pages it's left on, their numbers
/// kept, and gone when that's none; off pages it isn't on, nothing changes.
#[test]
fn a_removed_header_is_narrowed_or_gone() {
    let mut d = doc_of(3, &[]);
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    commit(&mut d, Actor::User, vec![fx::set_header(bates(), None)]).unwrap();
    commit(&mut d, Actor::User, vec![fx::remove_header(&[p1])]).unwrap();
    let g = drawn(&d);
    assert_eq!(g.scope, Scope::Pages([p0, p2].into_iter().collect()));
    assert_eq!(numbers(&d, &[p0, p1, p2]), [Some(100), None, Some(102)]);
    let n = d.version_count();
    commit(&mut d, Actor::User, vec![fx::remove_header(&[p1])]).unwrap();
    assert_eq!(d.version_count(), n);
    let all = Op::RemoveHeaderFooter {
        pages: PageSel::All,
        include_authored: false,
    };
    commit(&mut d, Actor::User, vec![all]).unwrap();
    assert!(d.state().header_footer().is_none());
}

/// Taken off some pages of every page, it's left on the others there are,
/// which reads the order: made before a page was inserted, it conflicts
/// with the insert, or lowered again, it's left on the inserted page too.
#[test]
fn a_removal_from_every_page_reads_the_order() {
    for stale in [Stale::Validate, Stale::Relower] {
        let mut d = doc_of(3, &[]);
        let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
        commit(&mut d, Actor::User, vec![fx::set_header(bates(), None)]).unwrap();
        let (v, mut tx) = planned(&d);
        tx.apply(fx::remove_header(&[p0]), &NoProgress).unwrap();
        let other = crate::store::blank(d.store(), 1);
        let inserted = fx::insert(&other, [0], "b.pdf", None);
        let c = d.commit_ops(vec![inserted], "Insert").unwrap();
        let new = c.created.pages().next().unwrap();
        match stale {
            Stale::Validate => {
                let e = d.commit(tx, on(v, stale)).unwrap_err();
                let conflict = EditError::Conflict {
                    key: Key::Order,
                    by: Some("Insert".into()),
                };
                assert_eq!(e, conflict);
            }
            Stale::Relower => {
                d.commit(tx, on(v, stale)).unwrap();
                let g = drawn(&d);
                assert_eq!(g.scope, Scope::Pages([p1, p2, new].into_iter().collect()));
                assert_eq!(
                    numbers(&d, &[p0, p1, p2, new]),
                    [None, Some(101), Some(102), None]
                );
            }
        }
    }
}

/// A header and footer on no page is refused; its records keep the
/// invariants: one at most, on pages there are, never none, and numbering
/// only pages it's on.
#[test]
fn headers_keep_the_invariants() {
    let mut d = doc_of(2, &[]);
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let nowhere = Op::SetHeaderFooter {
        spec: bates(),
        scope: ScopeSel::Pages(PageSel::Ids(Vec::new())),
    };
    let e = commit(&mut d, Actor::User, vec![nowhere]).unwrap_err();
    assert_eq!(
        e.to_string(),
        "set-header-footer: the header and footer is on no page"
    );
    commit(
        &mut d,
        Actor::User,
        vec![fx::set_header(bates(), Some(&[p0]))],
    )
    .unwrap();
    let st = d.state().clone();
    let g = drawn(&d);
    let with = |g: Generator| (Key::Generator(g.id), Some(Val::Generator(Arc::new(g))));
    let numbering = |pages: &[(PageId, u64)]| Frozen {
        bates: Some(Arc::new(pages.iter().copied().collect())),
        ..g.frozen.clone()
    };
    let pages = |ids: &[PageId]| Scope::Pages(ids.iter().copied().collect());
    let cases = [
        (
            Generator {
                scope: pages(&[]),
                ..Generator::clone(&g)
            },
            "is on no page".to_string(),
        ),
        (
            Generator {
                scope: pages(&[p0, PageId(99)]),
                ..Generator::clone(&g)
            },
            "is on PageId(99), which isn't a page".to_string(),
        ),
        (
            Generator {
                frozen: numbering(&[(p0, 100), (p1, 101)]),
                ..Generator::clone(&g)
            },
            format!("numbers {p1:?}, which it isn't on"),
        ),
        (
            Generator {
                scope: Scope::All,
                frozen: numbering(&[(PageId(99), 7)]),
                ..Generator::clone(&g)
            },
            "numbers PageId(99), which it isn't on".to_string(),
        ),
        (
            Generator {
                id: GenId(g.id.0 + 1),
                ..Generator::clone(&g)
            },
            "2 draw a header/footer".to_string(),
        ),
    ];
    for (broken, says) in cases {
        let (key, val) = with(broken);
        let mut st = st.clone();
        st.set(&key, val).unwrap();
        let why = st.check_invariants().unwrap_err().to_string();
        assert!(why.contains(&says), "{why}");
    }
}

/// Text its face can't draw is said when it's set (once a document); so is
/// a label it comes to draw, by new labels, that its face can't, and only
/// on a page it's on.
#[test]
fn a_label_its_face_cant_draw_is_said() {
    let mut d = doc_of(3, &[]);
    let [p0, _, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let set = fx::set_header(fx::header("<<label>>"), Some(&[p0]));
    let c = commit(&mut d, Actor::User, vec![set]).unwrap();
    assert!(c.notices.is_empty(), "{:?}", c.notices);
    let labels = |start| Op::SetPageLabels {
        ranges: Some(vec![fx::label(start, LabelStyle::Decimal, "Ω-", 1)]),
    };
    // From the third page: not on the header's page, which is "1".
    let c = commit(&mut d, Actor::User, vec![labels(p2)]).unwrap();
    assert!(c.notices.is_empty(), "{:?}", c.notices);
    let c = commit(&mut d, Actor::User, vec![labels(p0)]).unwrap();
    assert_eq!(c.notices, [Notice::Unencodable]);
    assert_eq!(d.state().label_of(p0).as_deref(), Some("Ω-1"));

    let mut d = doc_of(1, &[]);
    let set = fx::set_header(fx::header("Ωmega <<1>>"), None);
    let c = commit(&mut d, Actor::User, vec![set]).unwrap();
    assert_eq!(c.notices, [Notice::Unencodable]);
    let fine = fx::set_header(fx::header("Œuvre <<1>> <<label>>"), None);
    let mut d = doc_of(1, &[]);
    let c = commit(&mut d, Actor::User, vec![fine]).unwrap();
    assert!(c.notices.is_empty(), "{:?}", c.notices);
}

/// A progress that asks the op to stop once `at` pages are done, saying
/// how many pages there are to do.
#[derive(Default)]
struct StopAt {
    at: u64,
    done: AtomicU64,
    total: AtomicU64,
}

impl Progress for StopAt {
    fn set_total(&self, total: Option<u64>, _: Unit) {
        self.total.store(total.unwrap_or(0), Ordering::Relaxed);
    }
    fn set(&self, done: u64) {
        self.done.store(done, Ordering::Relaxed);
    }
    fn inc(&self, n: u64) {
        self.done.fetch_add(n, Ordering::Relaxed);
    }
    fn message(&self, _: Level, _: &str) {}
    fn is_cancelled(&self) -> bool {
        self.done.load(Ordering::Relaxed) >= self.at
    }
    fn child(&self, _: &str, _: f64) -> Box<dyn Progress> {
        Box::new(NoProgress)
    }
}

/// A removal reads each page's content, a page at a time, saying how many
/// there are; stopped part way, it leaves the transaction as it was, and
/// can go on.
#[test]
fn a_canceled_removal_leaves_the_transaction_as_it_was() {
    let mut d = doc_of(3, &[]);
    let all = [0, 1, 2].map(|i| d.page(i).unwrap());
    commit(&mut d, Actor::User, vec![fx::set_header(bates(), None)]).unwrap();
    let mut tx = d.tx(Actor::User);
    let state = tx.state().clone();
    let stop = StopAt {
        at: 2,
        ..StopAt::default()
    };
    let remove = fx::remove_header(&all);
    assert_eq!(tx.apply(remove.clone(), &stop), Err(EditError::Cancelled));
    assert_eq!(stop.total.load(Ordering::Relaxed), 3);
    assert_eq!(*tx.state(), state);
    assert!(tx.steps.is_empty());
    tx.apply(remove, &NoProgress).unwrap();
    assert!(tx.state().header_footer().is_none());
}

/// What [`others_on`] counts of a removal's is what the removal finds, and
/// what its line says: a page of Acrobat's header and watermark, one of the
/// author's running head and foot, one of the author's head no splice
/// takes, one of Acrobat's header no splice takes, and one that can't be
/// read; under a header of the document's on every page, which the count
/// takes off as the removal does. Of the headers and footers, and of the
/// watermarks, the author's taken or not.
#[test]
fn a_count_of_others_is_what_a_removal_finds() {
    use crate::materialize::header_tests::{ACROBAT_HEADER, AUTHORED};
    use crate::materialize::others_tests::{
        ACROBAT, ACROBAT_HEADER_UNSAFE, BODY, RUNNING, UNREAD, acrobat_file,
    };
    let file = acrobat_file(&[
        &[BODY, ACROBAT_HEADER, ACROBAT],
        &[RUNNING],
        &[BODY, AUTHORED],
        &[BODY, ACROBAT_HEADER_UNSAFE],
        &[UNREAD],
    ]);
    let mut d = super::tests::opened(file, "others.pdf");
    commit(&mut d, Actor::User, vec![fx::set_header(bates(), None)]).unwrap();
    let all: Vec<PageId> = d.state().order().iter().copied().collect();
    let headers = crate::generate::header::kinds();
    let watermarks = BTreeSet::from([PaginationKind::Watermark, PaginationKind::Background]);
    for include_authored in [false, true] {
        let ops = [
            Op::RemoveHeaderFooter {
                pages: fx::ids(&all),
                include_authored,
            },
            Op::RemoveWatermark {
                z: None,
                pages: fx::ids(&all),
                include_authored,
            },
        ];
        for (op, kinds) in ops.into_iter().zip([&headers, &watermarks]) {
            let counted = others_on(d.state(), kinds, &all, include_authored);
            let mut tx = d.tx(Actor::User);
            tx.apply(op, &NoProgress).unwrap();
            let found = match &*tx.steps[0].op {
                ResolvedOp::RemoveHeaderFooter { others, .. }
                | ResolvedOp::RemoveWatermark { others, .. } => *others,
                op => panic!("{op:?}"),
            };
            assert_eq!(counted, found, "{kinds:?}, {include_authored}");
        }
    }
    let counted = |include_authored| others_on(d.state(), &headers, &all, include_authored);
    let left = OthersFound {
        acrobat: 1,
        authored_left: 3,
        refused: 1,
        unreadable: 1,
        ..OthersFound::default()
    };
    assert_eq!(counted(false), left);
    let taken = OthersFound {
        authored: 2,
        authored_left: 1,
        ..left
    };
    assert_eq!(counted(true), taken);
    let watermark = OthersFound {
        acrobat: 1,
        unreadable: 1,
        ..OthersFound::default()
    };
    assert_eq!(others_on(d.state(), &watermarks, &all, false), watermark);
    let c = commit(&mut d, Actor::User, vec![fx::remove_header(&all)]).unwrap();
    assert_eq!(
        c.lines[0].text,
        "Remove the header and footer from pages 1–5 (1 made in Acrobat; 3 the author's left \
         in place; 1 left in place, as taking it out would change the page; 1 page that can't \
         be read left as it was)"
    );
}
