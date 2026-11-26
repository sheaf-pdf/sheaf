//! What PDFium gives of every page the content interpreter's tiers read
//! (`just tier-interp`: every form, the pool's sample, every scan and the
//! conformance suites' files that claim PDF/UA), drawn and read through
//! sheaf-render's engine, written as numbers and checksums only: a line a
//! file and a line a page. It asserts nothing. It is for setting one
//! build of PDFium, or of pdfium-render, or a change to how sheaf-render
//! has PDFium draw, beside what was there, over the same corpus, by
//! `diff`ing two runs' files. A tier compares sheaf with PDFium inside one
//! build, so most of what a build moves cancels out of it; this shows it.
//!
//! - **A file's line:** how PDFium opened it (`ok`, `password`,
//!   `refused:<why>`, `error:<why>`, or `unread:<why>` where the file
//!   couldn't be read), its page count, and its form's type.
//! - **A page's line:** its size; its characters, their count and one
//!   checksum of each one's code point, box and angle, in order; its
//!   text, its length and checksum; and `px=`, the page [`WIDTH`] pixels
//!   wide as the page view draws it ([`Draw::ALL`], BGRA), its size and a
//!   checksum of its pixels.
//! - **The other two ways sheaf-render has PDFium draw,** a column each
//!   on a file's first page alone: `mx=`, the page drawn with [`MATRIX`],
//!   which places it by a matrix and draws no widgets; and, for the forms
//!   manifest's files, `ap=`, the page as
//!   [`PdfiumEngine::render_appearances`] draws it (PDFium's flatten, then
//!   a render, RGBA). The flatten changes the engine's document, so `ap=`
//!   is taken last of its file.
//!
//! One thread, the files in their paths' order: PDFium's caches are the
//! process's, so what it draws can turn on what it drew before, and in one
//! order it draws the same, run to run. Two runs over the same code and
//! library write the same file. The log names the PDFium library the
//! process mapped and its SHA-256 (on Linux), so a run under another
//! library than the one meant shows there.
//!
//! `SHEAF_RENDER_DUMP=<file>` ([`DUMP`]) is where the lines go;
//! `SHEAF_RENDER_DUMP_ONLY=<corpus paths, comma-separated>` ([`ONLY`])
//! narrows the run to those files and prints their lines. In a whole run,
//! `SHEAF_RENDER_DUMP_RASTERS=<dir>` ([`RASTERS`]) keeps pages' renders as
//! PNG, `<path>-<page>.<column>.png` (the path's `/`s `_`s), for the pages
//! chosen one of two ways:
//! - `SHEAF_RENDER_DUMP_AGAINST=<an earlier run's file>` ([`AGAINST`]):
//!   each page whose `px=`, `mx=` or `ap=` isn't that file's, listed in
//!   `<dir>/pages.tsv`;
//! - `SHEAF_RENDER_DUMP_PAGES=<a file of lines: path, tab, page>`
//!   ([`PAGES`]): those pages, such as another run's `pages.tsv`.
//!
//! Never in a narrowed run: what PDFium draws after fewer files may differ.
//! A second ignored test, `SHEAF_RENDER_DUMP_COMPARE=<dirA>,<dirB>`
//! ([`COMPARE`]), sets each PNG of one directory beside the other's of the
//! same name: how many pixels differ, the most a channel differs by, and
//! the box they lie in.
//!
//! ```sh
//! just render-dump target/render.tsv
//! SHEAF_RENDER_DUMP_COMPARE=<dirA>,<dirB> cargo test -p sheaf-render --release \
//!   --test render_dump -- --ignored --nocapture --exact render_dump_compare
//! ```
//!
//! The test that isn't ignored holds the dump to what it says on a form
//! made here: one color of its first page's content changed moves that
//! page's `px=`, `mx=` and `ap=` and nothing else, a run against the
//! other keeps that page's renders alone, and the compare finds them
//! differing in that color's box. Needs libpdfium
//! (`scripts/fetch-pdfium.sh`), like the app itself.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use sheaf_doc::forms::corpus::Corpus;
use sheaf_doc::forms::fixture::Form;
use sheaf_doc::pdf::{Object, Stream, dictionary, literal};
use sheaf_edit::Rotation;
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Draw, FormType, Job, OpenError, PageIndex, Renderer};

/// How wide each page is drawn, in pixels: a pixel a point on a Letter
/// page.
const WIDTH: u32 = 612;

/// Where the dump's lines go: a path. Unset (and [`ONLY`] too), it reads
/// nothing. `just render-dump <file>` sets it.
const DUMP: &str = "SHEAF_RENDER_DUMP";

/// Corpus paths, comma-separated, the dump reads alone, printing their
/// lines.
const ONLY: &str = "SHEAF_RENDER_DUMP_ONLY";

/// A directory where a whole run keeps the renders of the pages
/// [`AGAINST`] or [`PAGES`] chooses, as PNG.
const RASTERS: &str = "SHEAF_RENDER_DUMP_RASTERS";

/// An earlier run's file: a run keeping rasters keeps those of each page
/// whose renders aren't that file's.
const AGAINST: &str = "SHEAF_RENDER_DUMP_AGAINST";

/// A file of pages, a line each (its path, a tab, its page from 1): a run
/// keeping rasters keeps those pages'.
const PAGES: &str = "SHEAF_RENDER_DUMP_PAGES";

/// Two directories of rasters, comma apart, for [`render_dump_compare`].
const COMPARE: &str = "SHEAF_RENDER_DUMP_COMPARE";

/// The matrix path's draw: annotations, and no widgets, which a render
/// placed by a matrix doesn't draw.
const MATRIX: Draw = Draw {
    annots: true,
    forms: false,
};

/// The columns of a page's line that are renders, what a run keeping
/// rasters chooses pages by.
const RENDERS: [&str; 3] = ["px=", "mx=", "ap="];

/// Every file the dump reads, by path, and the set that picks it: what
/// `tier-interp` reads (content_interp.rs's `picks`), every form, the
/// pool's sample, every scan and the conformance suites' files that claim
/// PDF/UA, each once, under the first set that names it.
fn picks(corpus: &Corpus) -> BTreeMap<String, &'static str> {
    let list = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/pool-sample.txt");
    let pool = std::fs::read_to_string(list).expect("bench/pool-sample.txt");
    let pool = pool.lines().map(str::trim).filter(|l| !l.is_empty());
    let forms = (corpus.forms().unwrap().into_iter()).map(|e| (e.path, "forms"));
    let pool = pool.map(|path| (path.to_string(), "pool sample"));
    let scans = (corpus.scans().unwrap().into_iter()).map(|e| (e.path, "scans"));
    let ua = (corpus.conformance().unwrap().into_iter())
        .filter(|e| e.column("claims").contains("PDF/UA"))
        .map(|e| (e.path, "PDF/UA"));
    let mut picks = BTreeMap::new();
    for (path, set) in forms.chain(pool).chain(scans).chain(ua) {
        picks.entry(path).or_insert(set);
    }
    picks
}

/// `said` on one line, to go in a column.
fn one_line(said: impl std::fmt::Display) -> String {
    said.to_string().replace(['\n', '\t'], " ")
}

/// An error of the engine's, its chain on one line.
fn why(e: &anyhow::Error) -> String {
    one_line(format!("{e:#}"))
}

/// The dump's word for a form's type. The dump names what it writes
/// itself, never by a type's `Debug`.
fn form_word(form: FormType) -> &'static str {
    match form {
        FormType::None => "none",
        FormType::AcroForm => "acroform",
        FormType::XfaForeground => "xfa-foreground",
        FormType::XfaFull => "xfa-full",
    }
}

/// The page's size, characters and text columns.
fn read_columns(engine: &PdfiumEngine, page: PageIndex) -> Vec<String> {
    let size = match engine.page_size(page) {
        Ok(size) => format!("size={}x{}", size.width, size.height),
        Err(e) => format!("size=err:{}", why(&e)),
    };
    let chars = match engine.page_chars(page) {
        Ok(chars) => {
            let mut sum = sheaf_util::Checksum::new();
            for c in &chars {
                sum.write(&u32::from(c.ch).to_le_bytes());
                match c.rect {
                    Some(r) => {
                        sum.write(&[1]);
                        for v in [r.x, r.y, r.w, r.h] {
                            sum.write(&v.to_bits().to_le_bytes());
                        }
                    }
                    None => sum.write(&[0]),
                }
                sum.write(&c.angle.to_bits().to_le_bytes());
            }
            format!("chars={}:{:032x}", chars.len(), sum.finish())
        }
        Err(e) => format!("chars=err:{}", why(&e)),
    };
    let text = match engine.page_text(page) {
        Ok(text) => format!(
            "text={}:{}",
            text.len(),
            sheaf_util::checksum_hex(text.as_bytes())
        ),
        Err(e) => format!("text=err:{}", why(&e)),
    };
    vec![size, chars, text]
}

/// A render's column, `name=<width>x<height>:<checksum of its pixels>`,
/// and the raster put in `kept` where rasters are kept.
fn drawn(
    name: &'static str,
    render: anyhow::Result<Raster>,
    kept: Option<&mut Vec<(&'static str, Raster)>>,
) -> String {
    match render {
        Ok(r) => {
            let column = format!(
                "{name}={}x{}:{}",
                r.width,
                r.height,
                sheaf_util::checksum_hex(&r.data)
            );
            if let Some(kept) = kept {
                kept.push((name, r));
            }
            column
        }
        Err(e) => format!("{name}=err:{}", why(&e)),
    }
}

/// The lines of the file at `path` in `bytes`: its own, then one a page
/// (the file's doc says what each holds), `ap=` on the first where
/// `form`. Each page's renders go to `keep`, if any, once its line is
/// whole.
fn dump_file(path: &str, bytes: Arc<[u8]>, form: bool, mut keep: Option<&mut Keep>) -> Vec<String> {
    let engine = match PdfiumEngine::open_bytes(bytes, None) {
        Ok(engine) => engine,
        Err(e) => {
            let how = match e.downcast_ref::<OpenError>() {
                Some(OpenError::Password) => "password".to_string(),
                Some(OpenError::Refused(why)) => format!("refused:{}", one_line(why)),
                None => format!("error:{}", why(&e)),
            };
            return vec![format!("{path}\t-\tfile\t{how}\tpages=-\tform=-")];
        }
    };
    let pages = engine.page_count();
    let form_type = form_word(engine.form_type());
    let mut lines = vec![format!(
        "{path}\t-\tfile\tok\tpages={pages}\tform={form_type}"
    )];
    let job = Job::Page {
        width: WIDTH,
        rotation: Rotation::R0,
    };
    // The first page's columns and renders, finished after the others.
    let mut first = None;
    for i in 0..pages {
        let page = PageIndex(i);
        let mut columns = read_columns(&engine, page);
        let mut rasters = Vec::new();
        let px = engine.render_with(page, job, PixelFormat::Bgra, Draw::ALL);
        columns.push(drawn("px", px, keep.is_some().then_some(&mut rasters)));
        if i == 0 {
            let mx = engine.render_with(page, job, PixelFormat::Bgra, MATRIX);
            columns.push(drawn("mx", mx, keep.is_some().then_some(&mut rasters)));
            first = Some((columns, rasters));
            // Its place, filled once its line is whole.
            lines.push(String::new());
            continue;
        }
        let line = format!("{path}\t{}\tpage\t{}", i + 1, columns.join("\t"));
        if let Some(keep) = keep.as_deref_mut() {
            keep.page(path, i + 1, &line, rasters);
        }
        lines.push(line);
    }
    if let Some((mut columns, mut rasters)) = first {
        if form {
            let ap = engine.render_appearances(PageIndex(0), WIDTH);
            columns.push(drawn("ap", ap, keep.is_some().then_some(&mut rasters)));
        }
        let line = format!("{path}\t1\tpage\t{}", columns.join("\t"));
        if let Some(keep) = keep {
            keep.page(path, 1, &line, rasters);
        }
        lines[1] = line;
    }
    lines
}

/// A page line's renders, its `px=`, `mx=` and `ap=` columns, tab apart.
fn renders(line: &str) -> String {
    let columns = line.split('\t').skip(3);
    let renders = columns.filter(|c| RENDERS.iter().any(|r| c.starts_with(r)));
    renders.collect::<Vec<_>>().join("\t")
}

/// A page's path and its page from 1, as its line says them.
type PageKey = (String, String);

/// The renders of each page of a dump's `text`, by page.
fn earlier_renders(text: &str) -> HashMap<PageKey, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let mut columns = line.split('\t');
        let (Some(path), Some(page), Some("page")) =
            (columns.next(), columns.next(), columns.next())
        else {
            continue;
        };
        out.insert((path.to_string(), page.to_string()), renders(line));
    }
    out
}

/// Whether the page of `line` draws otherwise than in `earlier`, or isn't
/// there.
fn moved_from(earlier: &HashMap<PageKey, String>, line: &str) -> bool {
    let mut columns = line.split('\t');
    let (Some(path), Some(page)) = (columns.next(), columns.next()) else {
        return true;
    };
    let key = (path.to_string(), page.to_string());
    earlier.get(&key) != Some(&renders(line))
}

/// The pages a list names, a line each: its path, a tab, its page from 1.
fn listed(text: &str) -> HashSet<PageKey> {
    let pages = text.lines().filter_map(|line| line.split_once('\t'));
    pages
        .map(|(path, page)| (path.into(), page.into()))
        .collect()
}

/// Which pages' renders a whole run keeps, and where.
struct Keep {
    dir: PathBuf,
    by: By,
    /// The pages kept, path and page tab apart, in the run's order.
    kept: Vec<String>,
}

enum By {
    /// Each page whose renders aren't these, an earlier run's.
    Against(HashMap<PageKey, String>),
    /// The pages listed.
    Pages(HashSet<PageKey>),
}

impl Keep {
    /// What [`RASTERS`], [`AGAINST`] and [`PAGES`] ask for, if anything:
    /// never in a run `narrowed` by [`ONLY`].
    fn from_env(narrowed: bool) -> Option<Keep> {
        let var = |name| std::env::var_os(name).map(PathBuf::from);
        let (dir, against, pages) = (var(RASTERS), var(AGAINST), var(PAGES));
        let Some(dir) = dir else {
            assert!(
                against.is_none() && pages.is_none(),
                "{AGAINST} and {PAGES} choose the pages {RASTERS} keeps, and it's unset"
            );
            return None;
        };
        assert!(
            !narrowed,
            "{RASTERS} keeps rasters in whole runs only: PDFium may draw otherwise after fewer files"
        );
        let read = |file: &Path| {
            std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()))
        };
        let by = match (against, pages) {
            (Some(against), None) => By::Against(earlier_renders(&read(&against))),
            (None, Some(pages)) => By::Pages(listed(&read(&pages))),
            _ => panic!("{RASTERS} keeps the pages one of {AGAINST} and {PAGES} chooses"),
        };
        Some(Keep::new(dir, by))
    }

    /// Keeping the renders of the pages `by` chooses in `dir`, made if
    /// it isn't there.
    fn new(dir: PathBuf, by: By) -> Keep {
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        Keep {
            dir,
            by,
            kept: Vec::new(),
        }
    }

    /// Page `page` of `path`, whose line is `line`: its `rasters` written
    /// where it's chosen.
    fn page(&mut self, path: &str, page: u32, line: &str, rasters: Vec<(&'static str, Raster)>) {
        let chosen = match &self.by {
            By::Against(earlier) => moved_from(earlier, line),
            By::Pages(pages) => pages.contains(&(path.to_string(), page.to_string())),
        };
        if !chosen {
            return;
        }
        for (name, raster) in rasters {
            let file = (self.dir).join(format!("{}-{page}.{name}.png", path.replace('/', "_")));
            let rgba = raster.into_rgba().expect("a whole raster");
            rgba.save(&file)
                .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        }
        self.kept.push(format!("{path}\t{page}"));
    }

    /// Says what was kept; after a run [`AGAINST`] another, lists it in
    /// `pages.tsv`.
    fn finish(&self) {
        match &self.by {
            By::Against(_) => {
                let list = self.dir.join("pages.tsv");
                let text: String = self.kept.iter().map(|page| format!("{page}\n")).collect();
                std::fs::write(&list, text).unwrap_or_else(|e| panic!("{}: {e}", list.display()));
                eprintln!(
                    "kept the renders of {} pages that moved, in {} (listed in {})",
                    self.kept.len(),
                    self.dir.display(),
                    list.display()
                );
            }
            By::Pages(pages) => eprintln!(
                "kept the renders of {} pages of the {} listed, in {}",
                self.kept.len(),
                pages.len(),
                self.dir.display()
            ),
        }
    }
}

/// Every PDFium library this process mapped, and its SHA-256, a line
/// each: so a run under another library than the one meant shows.
#[cfg(target_os = "linux")]
fn mapped_pdfium() -> Vec<String> {
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    let paths: BTreeSet<&str> = (maps.lines())
        .filter_map(|line| line.find('/').map(|at| &line[at..]))
        .filter(|path| {
            let name = Path::new(path).file_name().map(|n| n.to_string_lossy());
            name.is_some_and(|n| n.contains("pdfium"))
        })
        .collect();
    if paths.is_empty() {
        return vec!["none mapped".into()];
    }
    (paths.into_iter())
        .map(|path| {
            let sum = std::process::Command::new("sha256sum").arg(path).output();
            let sha = match sum {
                Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
                    .split_whitespace()
                    .next()
                    .unwrap_or("?")
                    .to_string(),
                Ok(out) => format!(
                    "unknown ({})",
                    one_line(String::from_utf8_lossy(&out.stderr))
                ),
                Err(e) => format!("unknown ({e})"),
            };
            format!("{path} sha256 {sha}")
        })
        .collect()
}

#[test]
#[ignore = "corpus dump: just render-dump <file>"]
fn render_dump() {
    let to = std::env::var_os(DUMP);
    let only = std::env::var(ONLY).ok();
    if to.is_none() && only.is_none() {
        eprintln!("skipped: neither {DUMP} nor {ONLY} is set");
        return;
    }
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let mut picks = picks(&corpus);
    if let Some(only) = &only {
        let named: BTreeSet<&str> = (only.split(',').map(str::trim))
            .filter(|p| !p.is_empty())
            .collect();
        assert!(!named.is_empty(), "{ONLY} is set but names no file");
        picks.retain(|path, _| named.contains(path.as_str()));
        assert_eq!(
            picks.len(),
            named.len(),
            "{ONLY} names a file the dump doesn't read: {only}"
        );
    }
    let mut keep = Keep::from_env(only.is_some());
    let mut sets: BTreeMap<&str, usize> = BTreeMap::new();
    for set in picks.values() {
        *sets.entry(set).or_default() += 1;
    }
    eprintln!("{} files: {sets:?}; pages {WIDTH} px wide", picks.len());

    let started = Instant::now();
    let mut lines = Vec::new();
    for (n, (path, set)) in picks.iter().enumerate() {
        let read = || match sheaf_util::read_file(&corpus.root.join(path)) {
            Ok(bytes) => dump_file(path, bytes, *set == "forms", keep.as_mut()),
            Err(e) => vec![format!(
                "{path}\t-\tfile\tunread:{}\tpages=-\tform=-",
                one_line(e)
            )],
        };
        let read = catch_unwind(AssertUnwindSafe(read)).unwrap_or_else(|panic| {
            let said = (panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .or_else(|| panic.downcast_ref::<String>().cloned());
            let said = said.unwrap_or_else(|| "a panic".into());
            vec![format!("{path}\t-\tpanicked\t{}", one_line(said))]
        });
        lines.extend(read);
        if (n + 1) % 500 == 0 {
            eprintln!(
                "{} of {} files, {:.0} s",
                n + 1,
                picks.len(),
                started.elapsed().as_secs_f64()
            );
        }
    }

    let pages = lines
        .iter()
        .filter(|l| l.split('\t').nth(2) == Some("page"));
    let not_ok = (lines.iter())
        .filter(|l| l.split('\t').nth(2) != Some("page") && !l.contains("\tfile\tok\t"))
        .count();
    eprintln!(
        "{} files, {} pages, {not_ok} not opened, in {:.0} s",
        picks.len(),
        pages.count(),
        started.elapsed().as_secs_f64()
    );
    #[cfg(target_os = "linux")]
    for library in mapped_pdfium() {
        eprintln!("library: {library}");
    }
    #[cfg(not(target_os = "linux"))]
    eprintln!("library: not read on this system");
    if let Some(keep) = &keep {
        keep.finish();
    }
    if only.is_some() {
        for line in &lines {
            eprintln!("{line}");
        }
    }
    if let Some(to) = to {
        let mut text = lines.join("\n");
        text.push('\n');
        std::fs::write(&to, text).unwrap_or_else(|e| panic!("{}: {e}", to.display()));
    }
}

/// The PNGs of a directory, by name.
fn pngs(dir: &Path) -> BTreeSet<String> {
    let read = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    (read.map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()))
        .filter(|name| name.ends_with(".png"))
        .collect()
}

/// Each PNG of `a` set beside `b`'s of the same name, a line each: its
/// name, its size, how many pixels differ, the most a channel differs by,
/// and the box they lie in (`x0,y0,x1,y1`, the ends past it), or that the
/// sizes differ, or that one directory alone has it; and a line of totals.
fn compared(a: &Path, b: &Path) -> Vec<String> {
    let (in_a, in_b) = (pngs(a), pngs(b));
    let mut lines = Vec::new();
    for name in in_a.difference(&in_b) {
        lines.push(format!("{name}\tonly in {}", a.display()));
    }
    for name in in_b.difference(&in_a) {
        lines.push(format!("{name}\tonly in {}", b.display()));
    }
    lines.push("name\tsize\tdiffering\tmost\tbox".into());
    let (mut same, mut differ) = (0, 0);
    for name in in_a.intersection(&in_b) {
        let open = |dir: &Path| {
            let file = dir.join(name);
            let image = image::open(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            image.to_rgba8()
        };
        let (pa, pb) = (open(a), open(b));
        if pa.dimensions() != pb.dimensions() {
            let ((wa, ha), (wb, hb)) = (pa.dimensions(), pb.dimensions());
            lines.push(format!("{name}\t{wa}x{ha} beside {wb}x{hb}\t-\t-\t-"));
            differ += 1;
            continue;
        }
        let (mut count, mut most) = (0usize, 0u8);
        let mut bounds = [u32::MAX, u32::MAX, 0, 0];
        for (x, y, p) in pa.enumerate_pixels() {
            let q = pb.get_pixel(x, y);
            let by = (p.0.iter().zip(q.0)).map(|(&s, t)| s.abs_diff(t)).max();
            let by = by.unwrap_or(0);
            if by > 0 {
                count += 1;
                most = most.max(by);
                bounds = [
                    bounds[0].min(x),
                    bounds[1].min(y),
                    bounds[2].max(x + 1),
                    bounds[3].max(y + 1),
                ];
            }
        }
        let (w, h) = pa.dimensions();
        if count == 0 {
            same += 1;
            lines.push(format!("{name}\t{w}x{h}\t0\t0\t-"));
        } else {
            differ += 1;
            let [x0, y0, x1, y1] = bounds;
            lines.push(format!(
                "{name}\t{w}x{h}\t{count}\t{most}\t{x0},{y0},{x1},{y1}"
            ));
        }
    }
    lines.push(format!(
        "{} pairs: {same} the same, {differ} differ; {} in one directory alone",
        same + differ,
        in_a.symmetric_difference(&in_b).count()
    ));
    lines
}

#[test]
#[ignore = "two runs' rasters side by side: SHEAF_RENDER_DUMP_COMPARE=<dirA>,<dirB>"]
fn render_dump_compare() {
    let Ok(dirs) = std::env::var(COMPARE) else {
        eprintln!("skipped: {COMPARE} is unset");
        return;
    };
    let (a, b) = (dirs.split_once(',')).unwrap_or_else(|| panic!("{COMPARE}=<dirA>,<dirB>"));
    for line in compared(Path::new(a.trim()), Path::new(b.trim())) {
        eprintln!("{line}");
    }
}

/// A form of two pages, as [`Form::new`] makes them, but that the first
/// draws a box filled in `color` below its "Page 1" (in black), and has a
/// text field with an appearance of its own.
fn form_in(color: &str) -> Vec<u8> {
    let mut form = Form::new(2);
    let content =
        format!("{color} rg 72 500 200 100 re f 0 g BT /F1 18 Tf 72 740 Td (Page 1) Tj ET");
    let content = form
        .pdf
        .add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let page = form.pages[0];
    form.dict(page).set("Contents", content);
    let field = form.text("name", 0, [100.0, 300.0, 300.0, 330.0]);
    form.dict(field).set("V", literal("Sheaf"));
    let ap = form.pdf.add_object(Stream::new(
        dictionary! {
            "Type" => Object::name("XObject"), "Subtype" => Object::name("Form"),
            "BBox" => vec![0.into(), 0.into(), 200.into(), 30.into()],
        },
        b"1 0 0 rg 0 0 200 30 re f".to_vec(),
    ));
    form.dict(field).set("AP", dictionary! { "N" => ap });
    form.bytes()
}

/// A line's columns by name: `size`, `px` and the rest, and `path`,
/// `page` and `kind` for its first three.
fn columns(line: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (i, column) in line.split('\t').enumerate() {
        let (name, value) = match (i, column.split_once('=')) {
            (0, _) => ("path", column),
            (1, _) => ("page", column),
            (2, _) => ("kind", column),
            (_, Some((name, value))) => (name, value),
            (_, None) => ("how", column),
        };
        out.insert(name.to_string(), value.to_string());
    }
    out
}

#[test]
fn a_color_of_the_content_moves_its_renders_and_nothing_else() {
    let dump = |color| dump_file("made.pdf", form_in(color).into(), true, None);
    let (blue, green) = (dump("0 0 1"), dump("0 0.5 0"));
    assert_eq!(blue.len(), 3, "a file line and two pages: {blue:#?}");
    assert_eq!(blue[0], "made.pdf\t-\tfile\tok\tpages=2\tform=acroform");
    assert_eq!(green[0], blue[0]);

    let (first, again) = (columns(&blue[1]), columns(&green[1]));
    let named: Vec<&str> = first.keys().map(String::as_str).collect();
    assert_eq!(
        named,
        [
            "ap", "chars", "kind", "mx", "page", "path", "px", "size", "text"
        ],
        "{}",
        blue[1]
    );
    assert!(!blue[1].contains("err:"), "{}", blue[1]);
    assert_eq!(first["size"], "612x792");
    assert_eq!(first["px"].split(':').next(), Some("612x792"));
    assert_eq!(first["text"].split(':').next(), Some("6"), "Page 1");
    let moved: Vec<&str> = (first.iter())
        .filter(|(name, value)| again.get(*name) != Some(value))
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(moved, ["ap", "mx", "px"], "{}\n{}", blue[1], green[1]);

    // The second page is the same, and has neither the matrix path's
    // render nor the flatten's.
    assert_eq!(green[2], blue[2]);
    let second = columns(&blue[2]);
    assert!(!second.contains_key("mx") && !second.contains_key("ap"));
    assert!(second.contains_key("px"), "{}", blue[2]);

    // A run against the blue one's file chooses the first page alone and
    // keeps its three renders; one run by that run's list keeps them too;
    // and the two sets differ in the blue box alone, every pixel of it
    // (72 to 272 across, 192 to 292 down), by all a channel can.
    let earlier = earlier_renders(&blue.join("\n"));
    assert!(moved_from(&earlier, &green[1]));
    assert!(!moved_from(&earlier, &green[2]));
    assert!(!moved_from(&earlier, &blue[1]));
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("render_dump");
    let _ = std::fs::remove_dir_all(&dir);
    let mut against = Keep::new(dir.join("green"), By::Against(earlier));
    let again = dump_file(
        "made.pdf",
        form_in("0 0.5 0").into(),
        true,
        Some(&mut against),
    );
    assert_eq!(again, green, "keeping rasters moves no line");
    against.finish();
    assert_eq!(against.kept, ["made.pdf\t1"]);
    let list = std::fs::read_to_string(dir.join("green/pages.tsv")).unwrap();
    assert_eq!(list, "made.pdf\t1\n");
    let mut by_list = Keep::new(dir.join("blue"), By::Pages(listed(&list)));
    dump_file(
        "made.pdf",
        form_in("0 0 1").into(),
        true,
        Some(&mut by_list),
    );
    assert_eq!(by_list.kept, ["made.pdf\t1"]);
    let lines = compared(&dir.join("blue"), &dir.join("green"));
    let box_of = |name: &str| format!("{name}\t612x792\t20000\t255\t72,192,272,292");
    let expected = [
        "name\tsize\tdiffering\tmost\tbox".to_string(),
        box_of("made.pdf-1.ap.png"),
        box_of("made.pdf-1.mx.png"),
        box_of("made.pdf-1.px.png"),
        "3 pairs: 0 the same, 3 differ; 0 in one directory alone".to_string(),
    ];
    assert_eq!(lines, expected);
}
