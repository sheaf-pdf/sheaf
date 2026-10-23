//! Combine Files (File > Create > Combine Files…, `sheaf combine`): one
//! new PDF from PDFs and images, each with an optional page subset, with a
//! bookmark per file and each PDF's own bookmarks nested under it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use in_progress::{NoProgress, Progress, Unit};
use serde::{Deserialize, Serialize};
use sheaf_doc::materialize::AskAnswers;
use sheaf_doc::tx::Made;
use sheaf_doc::{Actor, CommitOpts, FlatOutlineItem, JobKind, Notice, OutlineId, PageId, Tx};
use sheaf_edit::{
    BlobSel, BookmarkRef, Bring, DestView, Op, OutlineBring, PageRef, PageSel, PageSource,
    RangeSet, Sibling,
};

use crate::images::Paper;
use crate::session::{Decided, SaveTo, Session};

/// One input: a file and, optionally, which of its pages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CombineItem {
    pub path: PathBuf,
    /// A page range like "1-3, 7, 10-"; none for every page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl CombineItem {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            pages: None,
            password: None,
        }
    }

    /// A command-line argument: `file` or `file:range`. A path that exists
    /// as given wins, so `C:\scans\a.pdf` and odd file names still work.
    pub fn parse_arg(arg: &str) -> Self {
        if !Path::new(arg).exists()
            && let Some((path, range)) = arg.rsplit_once(':')
            && !path.is_empty()
            && !range.contains(['/', '\\'])
        {
            return Self {
                path: path.into(),
                pages: Some(range.to_string()),
                password: None,
            };
        }
        Self::new(arg)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct CombineParams {
    /// A bookmark per file, titled with its name, holding its own bookmarks.
    pub bookmark_each_file: bool,
    /// Page size for images.
    pub image_page: Paper,
}

impl Default for CombineParams {
    fn default() -> Self {
        Self {
            bookmark_each_file: true,
            image_page: Paper::Original,
        }
    }
}

#[derive(Debug, Default)]
pub struct CombineReport {
    pub pages: usize,
    pub files: usize,
    pub bookmarks: usize,
    /// About the forms combined: fields put under a new parent since their
    /// names were taken, and scripts turned off (M10 proposal 5).
    pub notice: Option<String>,
}

/// Whether `path` is a PDF (by its first bytes, else its extension).
pub fn is_pdf(path: &Path) -> bool {
    let mut head = [0u8; 5];
    match std::fs::File::open(path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)) {
        Ok(()) => &head == b"%PDF-",
        Err(_) => path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf")),
    }
}

/// Build the combined document (not yet written): the first file opened as
/// the document, or a document of no pages when it's an image, and the
/// rest put in by one transaction (M10.5 §14.2, proposal 35): the first
/// file's outline and labels taken off and its pages left out deleted,
/// each further file's pages inserted, with its marks and form, and a
/// bookmark per file over each file's own. The first file stays the
/// primary, so its catalog flows through, and so do its generators: a
/// header or watermark it has on every page is on the pages inserted too,
/// as an insert into it would have it. The history starts at the
/// combined version, with nothing to undo.
pub fn build(
    items: &[CombineItem],
    params: &CombineParams,
    progress: &dyn Progress,
) -> Result<(Session, CombineReport)> {
    let Some(first) = items.first() else {
        bail!("nothing to combine");
    };
    progress.set_total(Some(items.len() as u64), Unit::ITEMS);
    let pdf_first = is_pdf(&first.path);
    let mut session = match pdf_first {
        true => Session::open(&first.path, first.password.as_deref(), &NoProgress)
            .with_context(|| format!("opening {}", first.path.display()))?,
        false => Session::new_empty(),
    };
    let store = session.doc.store().clone();
    let mut tx = session.tx(Actor::Job(JobKind::Insert));
    if pdf_first {
        opened_aside(&mut tx)?;
    }
    let mut report = CombineReport::default();
    for (n, item) in items.iter().enumerate() {
        progress.check()?;
        let name = sheaf_doc::store::file_name(&item.path).to_string();
        // Page ids of this file's pages, by index in the file, and its
        // bookmarks.
        let (ids, outline) = match (n, pdf_first) {
            (0, true) => {
                let primary = session.doc.primary().clone();
                let wanted = wanted(item, primary.page_count(), &name)?;
                let all: Vec<PageId> = tx.state().order().iter().copied().collect();
                let drop: Vec<PageId> = (all.iter().enumerate())
                    .filter(|(i, _)| !wanted.contains(&(*i as u32)))
                    .map(|(_, id)| *id)
                    .collect();
                if !drop.is_empty() {
                    let delete = Op::DeletePages {
                        pages: PageSel::ids(drop),
                    };
                    tx.apply(delete, &NoProgress)?;
                }
                let ids = wanted.iter().map(|&i| (i, all[i as usize])).collect();
                (ids, primary.outline().clone())
            }
            _ if is_pdf(&item.path) => {
                let (src, _) = (store.read_file(&item.path, item.password.as_deref()))
                    .with_context(|| format!("opening {name}"))?;
                let wanted = wanted(item, src.page_count(), &name)?;
                // Its pages as one insert, its form as one instance of its
                // own (M10.5 §3.7), its marks with them, holding what they
                // hold.
                for blob in
                    sheaf_doc::tx::ready_to_insert(&store, session.doc.primary(), &src, &wanted)
                {
                    tx.pin(blob);
                }
                tx.pin(src.blob().clone());
                let from = PageSource::File {
                    file: BlobSel::Id(src.id()),
                    name: name.clone(),
                };
                let ids = insert(&mut tx, from, Some(&wanted))?;
                (ids, src.outline().clone())
            }
            _ => {
                // An image's pages are made in memory, as an insert of it
                // makes them (M10.5 §3.9).
                let image = store.read_image(&item.path)?;
                tx.pin(image.clone());
                let fit = params.image_page.fit(0.0);
                // A subset of a TIFF's pages needs their count, which only
                // reading them says.
                let wanted = match &item.pages {
                    Some(r) if !r.trim().is_empty() => {
                        let made = sheaf_doc::memory::images(&store, &[&**image.bytes()], fit)
                            .with_context(|| format!("reading {}", item.path.display()))?;
                        Some(wanted(item, made.page_count(), &name)?)
                    }
                    _ => None,
                };
                let from = PageSource::Images {
                    images: vec![BlobSel::Id(image.id())],
                    fit,
                };
                (insert(&mut tx, from, wanted.as_deref())?, Arc::from([]))
            }
        };
        let title = item
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or(name);
        report.bookmarks +=
            add_bookmarks(&mut tx, &title, &ids, &outline, params.bookmark_each_file)?;
        report.pages += ids.len();
        report.files += 1;
        progress.set(n as u64 + 1);
    }
    let mut notices = session
        .doc
        .commit(tx, CommitOpts::labelled("Combine"))?
        .notices;
    // What the new document starts from: there's nothing to undo to.
    session.doc.forget_before_cursor();
    // Told as one: the forms renamed, then the signatures that don't hold,
    // then the rest.
    notices.sort_by_key(|n| match n {
        Notice::FormRenamed { .. } | Notice::CopiesRenamed { .. } => 0,
        Notice::SignatureNotValidHere { .. } => 1,
        Notice::ScriptsOff | Notice::Unencodable => 2,
    });
    report.notice = sheaf_doc::notice_text(&notices);
    Ok((session, report))
}

/// The opened first file's own bookmarks and labels taken off in `tx`:
/// its bookmarks go under its own below, as every file's do, and the
/// combined file's pages are numbered as they come.
fn opened_aside(tx: &mut Tx) -> Result<()> {
    let roots: Vec<BookmarkRef> = (tx.state().outline_roots().iter())
        .map(|&id| BookmarkRef::Id(id))
        .collect();
    if !roots.is_empty() {
        tx.apply(Op::DeleteBookmarks { bookmarks: roots }, &NoProgress)?;
    }
    if tx.state().label_ranges().is_some() {
        tx.apply(Op::SetPageLabels { ranges: None }, &NoProgress)?;
    }
    Ok(())
}

/// The pages of a file of `count` pages that `item` names, from 0.
fn wanted(item: &CombineItem, count: u32, name: &str) -> Result<Vec<u32>> {
    Ok(match &item.pages {
        Some(r) if !r.trim().is_empty() => crate::pages::parse(r, count as usize)
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("{name}: pages"))?
            .into_iter()
            .map(|i| i as u32)
            .collect(),
        _ => (0..count).collect(),
    })
}

/// Pages `wanted` (from 0; every page when None) of `from` inserted at the
/// end in `tx`: each page's index in its file, with the page it made.
fn insert(tx: &mut Tx, from: PageSource, wanted: Option<&[u32]>) -> Result<Vec<(u32, PageId)>> {
    let pages = wanted
        .map(|w| RangeSet::from_runs(w.iter().map(|&i| i + 1..=i + 1)))
        .transpose()?;
    let insert = Op::InsertPages {
        from,
        pages,
        before: None,
        bring: Bring {
            outline: OutlineBring::None,
            ..Bring::default()
        },
        locals: Vec::new(),
    };
    let made = tx.apply(insert, &NoProgress)?.made;
    let pages = made.into_iter().filter_map(|m| match m {
        Made::Page(id) => Some(id),
        _ => None,
    });
    Ok(match wanted {
        Some(wanted) => wanted.iter().copied().zip(pages).collect(),
        None => (0..).zip(pages).collect(),
    })
}

/// This file's bookmarks at the end of the outline: under one titled with
/// the file's name (when `per_file`), else at the top level. Bookmarks to
/// pages left out are dropped unless something under them stays, which
/// makes them headings. Returns how many were added.
fn add_bookmarks(
    tx: &mut Tx,
    title: &str,
    ids: &[(u32, PageId)],
    outline: &[FlatOutlineItem],
    per_file: bool,
) -> Result<usize> {
    let target =
        |page: Option<u32>| page.and_then(|p| ids.iter().find(|(i, _)| *i == p).map(|(_, id)| *id));
    // Keep an item if it points at a kept page or a descendant does.
    let mut keep = vec![false; outline.len()];
    for i in (0..outline.len()).rev() {
        let child_kept = (i + 1..outline.len())
            .take_while(|&j| outline[j].depth > outline[i].depth)
            .any(|j| keep[j]);
        keep[i] = child_kept || target(outline[i].page).is_some();
    }
    let mut added = 0;
    let top = match per_file {
        true => {
            added += 1;
            let first = ids.first().map(|(_, id)| *id);
            Some(add_bookmark(tx, None, title.to_string(), first, None)?)
        }
        false => None,
    };
    // The kept bookmarks above this one, by their depth in the file: a
    // dropped parent's children move up a level.
    let mut stack: Vec<(u16, OutlineId)> = Vec::new();
    for (i, o) in outline.iter().enumerate() {
        while stack.last().is_some_and(|&(d, _)| d >= o.depth) {
            stack.pop();
        }
        if !keep[i] {
            continue;
        }
        let parent = stack.last().map(|&(_, id)| id).or(top);
        let t = target(o.page);
        added += 1;
        let id = add_bookmark(tx, parent, o.title.clone(), t, t.and(o.view.clone()))?;
        stack.push((o.depth, id));
    }
    Ok(added)
}

/// A bookmark titled `title`, last under `parent` (at the top level when
/// none), going to `view` on `target`: the one `tx` made.
fn add_bookmark(
    tx: &mut Tx,
    parent: Option<OutlineId>,
    title: String,
    target: Option<PageId>,
    view: Option<DestView>,
) -> Result<OutlineId> {
    let op = Op::AddBookmark {
        parent: parent.map(BookmarkRef::Id),
        at: Sibling::Last,
        title,
        target: target.map(PageRef::Id),
        view,
        color: None,
        style: Default::default(),
        local: None,
    };
    let made = tx.apply(op, &NoProgress)?.made;
    match made[..] {
        [Made::Bookmark(id)] => Ok(id),
        _ => bail!("adding a bookmark made {made:?}"),
    }
}

/// Combine `items` into a new PDF at `out`, written whole: `answers`
/// answer what the save asks (the first file's signatures it breaks, an
/// XFA part's values it drops). The combined document, saved there.
pub fn combine(
    items: &[CombineItem],
    params: &CombineParams,
    out: &Path,
    answers: &AskAnswers,
    progress: &dyn Progress,
) -> Result<(Session, CombineReport)> {
    let (mut session, report) = build(items, params, &*progress.child("reading", 0.7))?;
    progress.check()?;
    let to = SaveTo::File {
        path: out.to_path_buf(),
        append: false,
    };
    session
        .save(to, answers, &*progress.child("writing", 0.3))
        .decided()?;
    Ok((session, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each file's page stamps and attachments come with its marks: the
    /// combined document finds them in the files, whose stems nothing else
    /// holds, and writes them, and every mark reopens as sheaf's.
    #[test]
    fn marks_bring_what_they_hold() {
        use sheaf_doc::forms::fixture::{self, Form};
        use sheaf_doc::serialize::built::{WriteCx, to_pdf_in};
        use sheaf_doc::store::blob_id;
        use sheaf_doc::{AnnotKind, FormRef, Mark};
        let dir = std::env::temp_dir().join(format!(
            "sheaf-combine-{}-{:x}",
            std::process::id(),
            fastrand::u64(..)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let every = fixture::every_mark_kind();
        let held = fixture::holding(&[every[17].clone(), every[23].clone()]);
        // The page stamp and the attachment, each file's named its own.
        let items = ["a", "b"].map(|name| {
            let mut marks = held.clone();
            let file = format!("{name} notes").into_bytes();
            if let AnnotKind::PageStamp { stamp, .. } = &mut marks[0].annot.kind {
                *stamp = format!("{name}-stamp");
            }
            if let AnnotKind::FileAttachment {
                file: stem, size, ..
            } = &mut marks[1].annot.kind
            {
                (*stem, *size) = (blob_id(&file).stem(), file.len() as u64);
            }
            let store = sheaf_doc::Store::new();
            let blob = store.intern(file.into(), sheaf_doc::store::BlobKind::Attachment);
            marks[1].file = Some(blob.unwrap().into());
            let mut f = Form::new(1);
            let mut cx = WriteCx::new();
            let mut annots = Vec::new();
            for a in &marks {
                let main = to_pdf_in(a, &mut f.pdf, &mut cx).main;
                annots.push(lopdf::Object::Reference(f.pdf.add_object(main)));
            }
            let page = f.pages[0];
            f.dict(page).set("Annots", annots);
            let path = dir.join(format!("{name}.pdf"));
            std::fs::write(&path, f.bytes()).unwrap();
            CombineItem {
                path,
                pages: None,
                password: None,
            }
        });
        let (session, _) = build(&items, &CombineParams::default(), &NoProgress).unwrap();
        let st = session.doc.state();
        for (name, &p) in ["a", "b"].iter().zip(st.order()) {
            let marks: Vec<&Mark> = st.mark_records_on(p).collect();
            let stamp = marks[0].stamp.as_ref();
            assert!(matches!(stamp, Some(FormRef::Base(_))), "{name}");
            let file = marks[1].file.as_ref().unwrap();
            assert_eq!(
                **file.bytes(),
                *format!("{name} notes").as_bytes(),
                "{name}"
            );
        }
        let out = dir.join("combined.pdf");
        sheaf_doc::save::rewrite_to_path(&session.doc, &out).unwrap();
        let back = sheaf_doc::open::file(&out, None).unwrap();
        let st = back.doc.state();
        for &p in st.order() {
            let marks: Vec<bool> = st.annots_on(p).map(|(_, a)| a.mark().is_some()).collect();
            assert_eq!(marks, [true, true], "two marks, none another tool's");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Images are pages made in memory, first or after a PDF: the
    /// document starts from an image's, and the next comes in as an insert
    /// of it, nothing read from or written to a cache.
    #[test]
    fn images_are_pages_made_in_memory() {
        use sheaf_doc::store::{BlobKind, MemoryKind, Origin};
        let dir = std::env::temp_dir().join(format!(
            "sheaf-combine-images-{}-{:x}",
            std::process::id(),
            fastrand::u64(..)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let image = |name: &str, width| {
            let path = dir.join(name);
            image::RgbImage::from_pixel(width, 48, image::Rgb([10, 200, 10]))
                .save(&path)
                .unwrap();
            CombineItem::new(path)
        };
        let pdf = dir.join("blank.pdf");
        std::fs::write(
            &pdf,
            sheaf_doc::memory::blank_pages(612.0, 792.0, 2).unwrap(),
        )
        .unwrap();
        let items = [
            image("first.png", 96),
            CombineItem::new(&pdf),
            image("last.png", 64),
        ];
        let (session, report) = build(&items, &CombineParams::default(), &NoProgress).unwrap();
        assert_eq!((report.pages, report.files), (4, 3));
        let st = session.doc.state();
        let origins: Vec<Origin> = (st.order().iter())
            .map(|&p| match st.def(p).unwrap().base.src.blob().kind() {
                BlobKind::Pdf { origin, .. } => origin.clone(),
                _ => panic!("a PDF"),
            })
            .collect();
        let made = Origin::Memory(MemoryKind::Images);
        assert_eq!(origins[0], made);
        assert!(matches!(&origins[1], Origin::File(path) if **path == *pdf));
        assert_eq!(origins[3], made);
        let titles: Vec<String> = (st.bookmarks_preorder().iter())
            .map(|(_, _, b)| b.title.to_string())
            .collect();
        assert_eq!(titles, ["first", "blank", "last"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The first file opens as the document, its generators with it: a
    /// header it has on every page is on the pages inserted after it too,
    /// as an insert into it would have it (M10.5 proposal 35). Its labels
    /// and its own bookmarks are taken off, its bookmarks under its own
    /// again, as every file's are.
    #[test]
    fn the_first_file_s_header_covers_the_pages_combined_after_it() {
        use sheaf_doc::tx::fixture::{bookmark, header, set_header};
        let dir = std::env::temp_dir().join(format!(
            "sheaf-combine-header-{}-{:x}",
            std::process::id(),
            fastrand::u64(..)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = sheaf_doc::Store::new();
        let mut first = sheaf_doc::Document::new(
            store.clone(),
            sheaf_doc::store::blank(&store, 2),
            "first.pdf".into(),
            None,
            &[],
            sheaf_doc::history::default_policy(),
        );
        let p0 = first.state().order()[0];
        let labels = vec![sheaf_edit::PageLabelRange {
            start: PageRef::Id(p0),
            style: sheaf_edit::LabelStyle::LowerRoman,
            prefix: String::new(),
            first: 1,
        }];
        let ops = vec![
            set_header(header("HEADED"), None),
            bookmark(None, None, "Opening", Some(p0)),
            Op::SetPageLabels {
                ranges: Some(labels),
            },
        ];
        first.commit_ops(ops, "set up").unwrap();
        let a = dir.join("first.pdf");
        sheaf_doc::save::rewrite_to_path(&first, &a).unwrap();
        let b = dir.join("second.pdf");
        std::fs::write(&b, sheaf_doc::memory::blank_pages(612.0, 792.0, 1).unwrap()).unwrap();
        let items = [CombineItem::new(&a), CombineItem::new(&b)];
        let (session, report) = build(&items, &CombineParams::default(), &NoProgress).unwrap();
        assert_eq!(report.pages, 3);
        let st = session.doc.state();
        let headers: Vec<_> = st.generators().values().collect();
        assert_eq!(headers.len(), 1);
        assert!(st.order().iter().all(|&p| headers[0].covers(p)));
        assert_eq!(st.label_ranges(), None);
        let titles: Vec<(u16, String)> = (st.bookmarks_preorder().iter())
            .map(|(depth, _, b)| (*depth, b.title.to_string()))
            .collect();
        let want = [(0, "first"), (1, "Opening"), (0, "second")];
        assert_eq!(titles, want.map(|(d, t)| (d, t.to_string())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn args_split_off_a_range() {
        let a = CombineItem::parse_arg("a.pdf:1-3");
        assert_eq!(a.path, PathBuf::from("a.pdf"));
        assert_eq!(a.pages.as_deref(), Some("1-3"));
        assert_eq!(CombineItem::parse_arg("scan.tif").pages, None);
        // A path with a colon in a directory stays whole.
        assert_eq!(
            CombineItem::parse_arg("C:\\scans\\b.png").path,
            PathBuf::from("C:\\scans\\b.png")
        );
    }
}
