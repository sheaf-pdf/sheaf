//! What the Watermark and Header & Footer dialogs share, each setting one
//! of the document's generators (M10.5 slices 16 and 17): the faces text is
//! drawn in, by family and style ([`FONTS`]); the pages a generator is on,
//! as the Pages field says them ([`scope_text`]); and the count of what
//! others drew of the generator's kinds on the dialog's pages, which
//! applying replaces and Remove takes off, by who drew them ([`Count`];
//! proposal 31, D61).

use std::collections::BTreeSet;

use gpui::{Context, Task};
use sheaf_doc::content::PaginationKind;
use sheaf_doc::model::Scope;
use sheaf_doc::tx::{OthersFound, others_on};
use sheaf_doc::{DocState, PageId, Snapshot, VersionId};
use sheaf_edit::FontChoice;

/// The faces text is drawn in, by family (a row) and style (a column).
pub(crate) const FAMILIES: [&str; 3] = ["Helvetica", "Times", "Courier"];
pub(crate) const STYLES: [&str; 4] = ["Regular", "Bold", "Italic", "Bold Italic"];
pub(crate) const FONTS: [[FontChoice; 4]; 3] = [
    [
        FontChoice::Helvetica,
        FontChoice::HelveticaBold,
        FontChoice::HelveticaOblique,
        FontChoice::HelveticaBoldOblique,
    ],
    [
        FontChoice::TimesRoman,
        FontChoice::TimesBold,
        FontChoice::TimesItalic,
        FontChoice::TimesBoldItalic,
    ],
    [
        FontChoice::Courier,
        FontChoice::CourierBold,
        FontChoice::CourierOblique,
        FontChoice::CourierBoldOblique,
    ],
];

/// `font`'s family and style, as [`FONTS`] has them.
pub(crate) fn font_at(font: FontChoice) -> (usize, usize) {
    (FONTS.iter().enumerate())
        .find_map(|(family, row)| Some((family, row.iter().position(|&f| f == font)?)))
        .unwrap_or((0, 1))
}

/// The pages `scope` names of `st`, as a dialog's Pages field says them:
/// "2-3, 5", or empty for all of them.
pub(crate) fn scope_text(st: &DocState, scope: &Scope) -> String {
    match scope {
        Scope::All => String::new(),
        Scope::Pages(pages) => {
            let mut at: Vec<usize> = pages.iter().filter_map(|&p| st.pos_of(p)).collect();
            at.sort_unstable();
            crate::insert_dialog::ranges_text(&at)
        }
    }
}

/// Which version a snapshot is, of which document.
fn version_of(snap: &Snapshot) -> (u64, VersionId) {
    (snap.instance, snap.version)
}

/// What others drew of a dialog's kinds on its pages of the version it
/// opened on (§6.5, D61), counted as Remove counts it ([`others_on`]):
/// asked to leave the author's own, as applying leaves them, and asked to
/// take them too.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Counted {
    /// The version counted.
    of: (u64, VersionId),
    /// What applying replaces and Remove takes off, and what they leave.
    others: OthersFound,
    /// What Remove takes off asked to take the author's own too.
    with_authored: OthersFound,
}

impl Counted {
    /// What others drew of `kinds` on `pages` of `snap`, counted. It reads
    /// each page's content, so it's for off the UI thread.
    pub(crate) fn of(snap: &Snapshot, kinds: &BTreeSet<PaginationKind>, pages: &[PageId]) -> Self {
        Counted {
            of: version_of(snap),
            others: others_on(&snap.state, kinds, pages, false),
            with_authored: others_on(&snap.state, kinds, pages, true),
        }
    }

    /// What a dialog says of it, of the pages `on` ("all pages", "pages
    /// 2-4"): "On all pages: 2 made in Acrobat and 1 by an older sheaf will
    /// be replaced; 12 the author's stay (Remove can take them)", with
    /// those a splice refuses, those inside forms and the pages that can't
    /// be read when there are any; or that others drew no `what` ("header
    /// or footer") there.
    pub(crate) fn line(&self, on: &str, what: &str) -> String {
        let OthersFound {
            acrobat,
            other_files,
            older_sheaf,
            authored_left,
            refused,
            in_forms,
            unreadable,
            ..
        } = self.others;
        let takes = self.with_authored.authored;
        let stay = |n: usize| if n == 1 { "stays" } else { "stay" };
        let them = |n: usize| if n == 1 { "it" } else { "them" };
        let replaced: Vec<String> = [
            (acrobat, "made in Acrobat"),
            (older_sheaf, "by an older sheaf"),
            (other_files, "from other files"),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, who)| format!("{n} {who}"))
        .collect();
        let mut said = Vec::new();
        if let Some((last, first)) = replaced.split_last() {
            let all = match first.is_empty() {
                true => last.clone(),
                false => format!("{} and {last}", first.join(", ")),
            };
            said.push(format!("{all} will be replaced"));
        }
        if authored_left > 0 {
            let can = match takes {
                0 => String::new(),
                n if n == authored_left => format!(" (Remove can take {})", them(n)),
                n => format!(" (Remove can take {n} of them)"),
            };
            let n = authored_left;
            said.push(format!("{n} the author's {}{can}", stay(n)));
        }
        if refused > 0 {
            said.push(format!(
                "{refused} {}, as taking {} out would change the page",
                stay(refused),
                them(refused)
            ));
        }
        if in_forms > 0 {
            said.push(format!("any inside forms on {} stay", pages(in_forms)));
        }
        if unreadable > 0 {
            said.push(format!("{} can't be read", pages(unreadable)));
        }
        match said.is_empty() {
            true => format!("On {on}: no {what} others drew"),
            false => format!("On {on}: {}", said.join("; ")),
        }
    }
}

/// `n` pages, in words.
fn pages(n: usize) -> String {
    format!("{n} page{}", if n == 1 { "" } else { "s" })
}

/// A dialog's [`Counted`], made once as the dialog opens, off the UI
/// thread, of the version it opened on.
pub(crate) struct Count {
    /// The version the dialog opened on, the one counted.
    of: (u64, VersionId),
    /// The count, once it's landed.
    counted: Option<Counted>,
    /// The counting, which closing the dialog drops.
    _task: Option<Task<()>>,
}

impl Count {
    /// A count of `opened`, the version a dialog opened on, still to land.
    fn new(opened: &Snapshot) -> Count {
        Count {
            of: version_of(opened),
            counted: None,
            _task: None,
        }
    }

    /// What others drew of `kinds` on `pages` of `opened`, the version the
    /// dialog `cx` is of opened on, counted off the UI thread and handed to
    /// the dialog's count, which `count` finds, as it lands.
    pub(crate) fn begin<D: 'static>(
        opened: &Snapshot,
        kinds: BTreeSet<PaginationKind>,
        pages: Vec<PageId>,
        count: fn(&mut D) -> &mut Count,
        cx: &mut Context<D>,
    ) -> Count {
        let snap = opened.clone();
        let task = cx.spawn(async move |this, cx| {
            let counting = cx
                .background_executor()
                .spawn(async move { Counted::of(&snap, &kinds, &pages) });
            let counted = counting.await;
            this.update(cx, |d, cx| {
                if count(d).take(counted) {
                    cx.notify();
                }
            })
            .ok();
        });
        Count {
            _task: Some(task),
            ..Count::new(opened)
        }
    }

    /// `counted` taken when it's of the version the dialog opened on; one
    /// of another is dropped. Whether it was taken.
    pub(crate) fn take(&mut self, counted: Counted) -> bool {
        if counted.of != self.of {
            return false;
        }
        self.counted = Some(counted);
        true
    }

    /// What the dialog says of it ([`Counted::line`]), or that it's still
    /// counting.
    pub(crate) fn said(&self, on: &str, what: &str) -> String {
        match &self.counted {
            Some(counted) => counted.line(on, what),
            None => "Counting…".into(),
        }
    }
}

/// The pages a dialog counts on, its selection's (0-based indexes, of more
/// than one page, as [`DocView::selected_indexes`] gives them) or all of
/// `opened`'s when it's empty, by id, and as its line names them.
///
/// [`DocView::selected_indexes`]: crate::docview::DocView::selected_indexes
pub(crate) fn counted_pages(opened: &Snapshot, selected: &[usize]) -> (Vec<PageId>, String) {
    match selected {
        [] => (
            opened.state.order().iter().copied().collect(),
            "all pages".into(),
        ),
        _ => (
            crate::page_insert::ids_at(opened, selected),
            format!("pages {}", crate::insert_dialog::ranges_text(selected)),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Document as Pdf, Object, Stream, dictionary};
    use sheaf_doc::Document;
    use sheaf_doc::generate::header;
    use sheaf_doc::model::WatermarkGen;
    use sheaf_edit::Z;

    /// A file of three Letter pages, a header on each: Acrobat's, the
    /// author's in a `q` of its own, which a splice takes out, and the
    /// author's with its font set outside one, as a word processor writes
    /// it, which no splice takes out.
    fn three_heads() -> Vec<u8> {
        let mut pdf = Pdf::with_version("1.7");
        let root = pdf.new_object_id();
        let font = pdf.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let piece = dictionary! { "ADBE_CompoundType" => dictionary! {
            "DocSettings" => dictionary! {}, "Private" => "Header",
        } };
        let form = pdf.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form", "PieceInfo" => piece,
                "BBox" => vec![0.into(), 0.into(), 300.into(), 60.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
            },
            b"BT /F1 12 Tf 2 10 Td (ACROBAT) Tj ET".to_vec(),
        ));
        let drawn: [&[u8]; 3] = [
            b"/Artifact <</Type /Pagination /Subtype /Header>> BDC \
              q 1 0 0 1 72 740 cm /Fm0 Do Q EMC",
            b"/Artifact <</Type /Pagination /Subtype /Header>> BDC \
              q BT /F1 9 Tf 72 760 Td (Chapter One) Tj ET Q EMC",
            b"/Artifact <</Type /Pagination /Subtype /Header>> BDC \
              BT /F1 9 Tf 72 760 Td (Chapter Two) Tj ET EMC",
        ];
        let kids: Vec<Object> = (drawn.into_iter())
            .map(|content| {
                let content = pdf.add_object(Stream::new(dictionary! {}, content.to_vec()));
                let page =
                    dictionary! { "Type" => "Page", "Parent" => root, "Contents" => content };
                pdf.add_object(page).into()
            })
            .collect();
        let pages = dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => 3,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => font },
                "XObject" => dictionary! { "Fm0" => form },
            },
        };
        pdf.objects.insert(root, Object::Dictionary(pages));
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
        pdf.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        pdf.save_to(&mut bytes).unwrap();
        bytes
    }

    /// `bytes` opened as a file.
    fn opened_as_file(bytes: &[u8]) -> (tempfile::TempDir, Document) {
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("heads.pdf");
        std::fs::write(&path, bytes).unwrap();
        let doc = sheaf_doc::open::file(&path, None).unwrap().doc;
        (dir, doc)
    }

    /// A dialog's count is what Remove finds of others' on its pages, by
    /// who drew them (proposal 31, D61): Acrobat's header, which applying
    /// replaces, and the author's two, which stay, Remove taking the one a
    /// splice can take out when asked to; on the selection's pages alone
    /// when there is one; none of another kind. It lands for the version
    /// the dialog opened on and no other: one counted on a version since,
    /// or of another document, is dropped, and until one lands the dialog
    /// says it's counting.
    #[test]
    fn a_count_is_of_the_version_the_dialog_opened_on() {
        let (_dir, mut doc) = opened_as_file(&three_heads());
        let opened = doc.snapshot();
        let kinds = header::kinds();
        let (all, on) = counted_pages(&opened, &[]);
        assert_eq!((all.len(), on.as_str()), (3, "all pages"));
        let counted = Counted::of(&opened, &kinds, &all);
        assert_eq!(
            counted.line(&on, "header or footer"),
            "On all pages: 1 made in Acrobat will be replaced; 2 the author's stay \
             (Remove can take 1 of them)"
        );
        let (last, on) = counted_pages(&opened, &[1, 2]);
        assert_eq!(
            Counted::of(&opened, &kinds, &last).line(&on, "header or footer"),
            "On pages 2-3: 2 the author's stay (Remove can take 1 of them)"
        );
        let watermarks = BTreeSet::from([WatermarkGen::kind_at(Z::Over)]);
        assert_eq!(
            Counted::of(&opened, &watermarks, &all).line("all pages", "watermark"),
            "On all pages: no watermark others drew"
        );

        let mut count = Count::new(&opened);
        assert_eq!(count.said("all pages", "header or footer"), "Counting…");
        let set = sheaf_doc::tx::fixture::set_header(sheaf_doc::tx::fixture::header("Mine"), None);
        doc.commit_ops(vec![set], "Header").unwrap();
        let since = Counted::of(&doc.snapshot(), &kinds, &all);
        assert!(!count.take(since), "a count of a version since is dropped");
        let (_other_dir, other) = opened_as_file(&three_heads());
        let elsewhere = Counted::of(&other.snapshot(), &kinds, &all);
        assert!(!count.take(elsewhere), "and one of another document");
        assert_eq!(count.said("all pages", "header or footer"), "Counting…");
        assert!(count.take(counted.clone()));
        assert_eq!(
            count.said("all pages", "header or footer"),
            counted.line("all pages", "header or footer")
        );
    }
}
