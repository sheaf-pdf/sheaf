//! Combine Files (File > Create > Combine Files…, `sheaf combine`): one
//! new PDF from PDFs and images, each with an optional page subset, with a
//! bookmark per file and each PDF's own bookmarks nested under it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use in_progress::{NoProgress, Progress, Unit};
use serde::{Deserialize, Serialize};
use sheaf_doc::materialize::Came;
use sheaf_doc::open::Facts;
use sheaf_doc::tx::Made;
use sheaf_doc::{
    Actor, CommitOpts, Document, FlatOutlineItem, JobKind, Notice, OutlineId, PageId, Store, Tx,
};
use sheaf_edit::{
    BlobSel, BookmarkRef, Bring, DestView, Op, OutlineBring, PageRef, PageSel, PageSource,
    RangeSet, Sibling,
};

use crate::images::Paper;

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

/// The file sheaf can take its pages from: the PDF itself, or a PDF made
/// from the image (kept in the generated-file cache).
pub fn page_source(path: &Path, image_page: Paper) -> Result<PathBuf> {
    if is_pdf(path) {
        return Ok(path.to_path_buf());
    }
    let pages = sheaf_image::io::read(path)?;
    let pdf = sheaf_doc::images::to_pdf(&pages, image_page.fit(0.0))?;
    sheaf_doc::generated::store(&pdf)
}

/// Build the combined document (not yet written): the first file opened
/// as the document, and the rest put in by one transaction (M10.5 §14.2):
/// the first file's pages left out deleted, each further file's pages
/// inserted, with its marks and form, and a bookmark per file.
pub fn build(
    items: &[CombineItem],
    params: &CombineParams,
    progress: &dyn Progress,
) -> Result<(Document, CombineReport)> {
    if items.is_empty() {
        bail!("nothing to combine");
    }
    progress.set_total(Some(items.len() as u64), Unit::ITEMS);
    let store = Store::new();
    // The first file's document, and the transaction the rest go into.
    let mut doc: Option<(Document, Tx)> = None;
    let mut report = CombineReport::default();
    for (n, item) in items.iter().enumerate() {
        progress.check()?;
        let name = item
            .path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let file = page_source(&item.path, params.image_page)
            .with_context(|| format!("reading {}", item.path.display()))?;
        let (src, stamp) = (store.read_file(&file, item.password.as_deref()))
            .with_context(|| format!("opening {name}"))?;
        let count = src.page_count() as usize;
        let wanted: Vec<u32> = match &item.pages {
            Some(r) if !r.trim().is_empty() => crate::pages::parse(r, count)
                .map_err(anyhow::Error::msg)
                .with_context(|| format!("{name}: pages"))?
                .into_iter()
                .map(|i| i as u32)
                .collect(),
            _ => (0..src.page_count()).collect(),
        };
        let outline = src.outline().clone();
        // Page ids of this file's pages, by index in the file.
        let ids: Vec<(u32, PageId)> = match doc.as_mut() {
            None => {
                // The first file opens as the document, with its marks and
                // form; an image's page has none. Its bookmarks go under its
                // own below, like every file's, and its labels and
                // header/footer aren't taken.
                let came = Came::Read {
                    path: file.as_path().into(),
                    stamp,
                };
                let facts = match is_pdf(&item.path) {
                    true => Facts {
                        annots: src.annots().clone(),
                        form: src.form().clone(),
                        ..Default::default()
                    },
                    false => Facts::default(),
                };
                let mut d = sheaf_doc::open::start(
                    store.clone(),
                    src,
                    sheaf_doc::store::file_name(&file),
                    Some(came),
                    sheaf_doc::history::default_policy(),
                );
                d.ready(facts)?;
                let mut tx = d.tx(Actor::Job(JobKind::Insert));
                let all: Vec<PageId> = d.state().order().iter().copied().collect();
                let drop: Vec<PageId> = all
                    .iter()
                    .enumerate()
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
                doc = Some((d, tx));
                ids
            }
            Some((d, tx)) => {
                // Its pages as one insert, its form as one instance of
                // its own (M10.5 §3.7), its marks with them, holding what
                // they hold.
                for blob in sheaf_doc::tx::ready_to_insert(d.store(), d.primary(), &src, &wanted) {
                    tx.pin(blob);
                }
                tx.pin(src.blob().clone());
                let insert = Op::InsertPages {
                    from: PageSource::File {
                        file: BlobSel::Id(src.id()),
                        name: sheaf_doc::store::file_name(&file).to_string(),
                    },
                    pages: Some(RangeSet::from_runs(wanted.iter().map(|&i| i + 1..=i + 1))?),
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
                wanted.iter().copied().zip(pages).collect()
            }
        };
        let (_, tx) = doc.as_mut().expect("made above");
        let title = item
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or(name);
        report.bookmarks += add_bookmarks(tx, &title, &ids, &outline, params.bookmark_each_file)?;
        report.pages += ids.len();
        report.files += 1;
        progress.set(n as u64 + 1);
    }
    let (mut doc, tx) = doc.expect("at least one item");
    let mut notices = doc.commit(tx, CommitOpts::labelled("Combine"))?.notices;
    // Told as one: the forms renamed, then the signatures that don't hold,
    // then the rest.
    notices.sort_by_key(|n| match n {
        Notice::FormRenamed { .. } | Notice::CopiesRenamed { .. } => 0,
        Notice::SignatureNotValidHere { .. } => 1,
        Notice::ScriptsOff | Notice::Unencodable => 2,
    });
    report.notice = sheaf_doc::notice_text(&notices);
    Ok((doc, report))
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

/// Combine `items` into a new PDF at `out`.
pub fn combine(
    items: &[CombineItem],
    params: &CombineParams,
    out: &Path,
    progress: &dyn Progress,
) -> Result<CombineReport> {
    let (doc, report) = build(items, params, &*progress.child("reading", 0.7))?;
    progress.check()?;
    sheaf_doc::save::rewrite_to_path(&doc.snapshot(), out)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each file's page stamps and attachments come with its marks: the
    /// combined document finds them in the files, whose stems no cache
    /// has, and writes them, and every mark reopens as sheaf's.
    #[test]
    fn marks_bring_what_they_hold() {
        use sheaf_doc::annot::{WriteCx, to_pdf_in};
        use sheaf_doc::forms::fixture::{self, Form};
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
        let (doc, _) = build(&items, &CombineParams::default(), &NoProgress).unwrap();
        let st = doc.state();
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
        sheaf_doc::save::rewrite_to_path(&doc.snapshot(), &out).unwrap();
        let back = sheaf_doc::open::file(&out, None).unwrap();
        let st = back.doc.state();
        for &p in st.order() {
            let marks: Vec<bool> = st.annots_on(p).map(|(_, a)| a.mark().is_some()).collect();
            assert_eq!(marks, [true, true], "two marks, none another tool's");
        }
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
