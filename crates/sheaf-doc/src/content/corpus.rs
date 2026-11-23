//! The content toolkit over Greg's corpus of real forms
//! ([`forms::corpus`](crate::forms::corpus)): every page's content, and
//! every form XObject it draws, reads, has tokens that tile it and
//! splices back to each stream's own bytes, read apart; its marked content
//! balances; its words decode through its fonts; its pagination artifacts are
//! classified as the tools that made the files drew them; each of them
//! splices out, as a strip cuts it, or is refused, Acrobat's never; and
//! each list of theirs written in the content can be written again without
//! a key, as Clean Copy writes sheaf's without what marks it.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::ControlFlow;
use std::path::Path;
use std::time::Instant;

use crate::pdf::{Object, ObjectId, StreamExt};

use super::interp::{
    Cap, EncodedPage, Error, Event, GfxError, ImageOf, Kind, Mark, MarkProps, Matrix, Opts, Rect,
    Refused, Seg, State, Summary, Unwritable, encode, interpret,
};
use super::tests::blank;
use super::*;
use crate::forms::corpus::{Corpus, Entry, each_entry};
use crate::pdf::Why;
use crate::pdf::gfx::{ActiveColorSpace, BlendState, RenderingIntent, SoftMask, SoftMaskKind};
use crate::source::Limits;
use crate::store::{BlobKind, Store};

const LIM: &Limits = &Limits::DEFAULT;

/// What one file's content came to.
#[derive(Default)]
struct Outcome {
    pages: usize,
    forms: usize,
    bytes: usize,
    /// Content that can't be read, or whose tokens stop or don't tile
    /// it, with its page: each a line.
    unread: Vec<String>,
    /// Pages whose marked content doesn't balance.
    unbalanced: Vec<String>,
    /// Streams the concatenation, or a splice of nothing, changed from
    /// their own bytes.
    changed: Vec<String>,
    origins: BTreeMap<(PaginationKind, PagOrigin), usize>,
    /// Pagination artifacts spliced out alone, as a strip cuts them, by
    /// who drew them and whether the splice was made; Acrobat's refused,
    /// and pages where splicing out together every artifact of a kind
    /// that a filter or a generator's `replaces` selects
    /// ([`PageBlocks::selected`]) is refused.
    splices: BTreeMap<(PagOrigin, bool), usize>,
    acrobat_refused: Vec<String>,
    unstrippable: Vec<String>,
    /// Pagination artifacts' lists written again without `/Type`, as a
    /// filter writes sheaf's without what marks it, by whether the
    /// list is written in the content and whether it could be; and those
    /// in the content refused, each a line.
    rewrites: BTreeMap<(bool, bool), usize>,
    unrewritable: Vec<String>,
    nested: usize,
    /// The words pages draw, decoded through their fonts.
    words: usize,
    /// The marked ranges blocks keep, and the most on a page.
    kept: usize,
    most_kept: usize,
    /// The kinds the file's bytes say Acrobat drew, those classified as
    /// Acrobat's, and those found at all.
    acrobat_said: BTreeSet<PaginationKind>,
    acrobat_found: BTreeSet<PaginationKind>,
    kinds_found: BTreeSet<PaginationKind>,
    /// Whether Word or LibreOffice made it.
    office: bool,
}

/// The pagination kinds `bytes` say Acrobat drew: `/ADBE_CompoundType`
/// dictionaries' `/Private` names, found by a scan.
fn acrobat_said(bytes: &[u8]) -> BTreeSet<PaginationKind> {
    let mut said = BTreeSet::new();
    let mut at = 0;
    while let Some(i) = find(&bytes[at..], b"ADBE_CompoundType") {
        at += i + 1;
        let near = &bytes[at..(at + 300).min(bytes.len())];
        if let Some(p) = find(near, b"/Private") {
            let name = near[p + 8..]
                .iter()
                .skip_while(|c| c.is_ascii_whitespace())
                .skip(1)
                .take_while(|c| c.is_ascii_alphanumeric())
                .copied()
                .collect::<Vec<_>>();
            said.extend(PaginationKind::from_name(&name));
        }
    }
    said
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Whether Word or LibreOffice made `src`, by its Info's `/Producer` or
/// `/Creator`, or its XMP.
fn office(src: &crate::store::Src) -> bool {
    let pdf = src.pdf();
    let made_by = |s: &[u8]| {
        let s = String::from_utf8_lossy(s);
        [
            "Microsoft® Word",
            "Microsoft Word",
            "LibreOffice",
            "OpenOffice",
        ]
        .iter()
        .any(|m| s.contains(m))
    };
    let info = pdf
        .trailer()
        .get(b"Info")
        .and_then(|i| pdf.dereference(i).ok())
        .and_then(|(_, i)| i.as_dict());
    let in_info = info.is_some_and(|info| {
        [b"Producer".as_slice(), b"Creator"].iter().any(|key| {
            info.get(key)
                .and_then(Object::as_bytes)
                .is_some_and(|v| made_by(&crate::file::text_string(v).into_bytes()))
        })
    });
    let in_xmp = || {
        let catalog = pdf.catalog().ok()?;
        let (_, xmp) = pdf.dereference(catalog.get(b"Metadata")?).ok()?;
        let xmp = xmp.as_stream()?;
        Some(made_by(&xmp.decompressed_content().ok()?))
    };
    in_info || in_xmp() == Some(true)
}

/// XObject `name` of `resources`.
fn xobject(src: &crate::store::Src, resources: Option<&Object>, name: &[u8]) -> Option<ObjectId> {
    let pdf = src.pdf();
    let (_, resources) = pdf.dereference(resources?).ok()?;
    let (_, xobjects) = pdf
        .dereference(resources.as_dict()?.get(b"XObject")?)
        .ok()?;
    xobjects.as_dict()?.get(name)?.as_reference()
}

/// Where `s`'s tokens, read with `res`, don't tile it: an error, a
/// token out of order, or something between or after them that isn't
/// white-space or a comment.
fn untiled(s: &[u8], res: &ResCx) -> Option<String> {
    let mut at = 0;
    for token in tokens(s, res, &LIM.content.family()) {
        let r = match token {
            Ok((r, _)) => r,
            Err(e) => return Some(e.to_string()),
        };
        if !(at <= r.start && r.start < r.end) || !blank(&s[at..r.start]) {
            return Some(format!("bytes {at} to {}", r.start));
        }
        at = r.end;
    }
    (!blank(&s[at..])).then(|| format!("bytes after {at}"))
}

fn read_file(corpus: &Corpus, entry: &Entry) -> Outcome {
    let mut out = Outcome::default();
    let path = corpus.path(entry);
    let Ok(bytes) = std::fs::read(&path) else {
        return out;
    };
    out.acrobat_said = acrobat_said(&bytes);
    let kind = BlobKind::file(Path::new(&entry.path), None);
    let Ok(src) = Store::new().open(bytes.into(), kind) else {
        return out;
    };
    out.office = office(&src);
    let mut forms_seen = HashSet::new();
    let known = DrawnForms::default();
    let fonts = Fonts::for_text();
    // Content is read with pdf-content-streams' limits.
    let cs = &LIM.content.family();
    for (i, page) in src.pages().iter().enumerate() {
        out.pages += 1;
        let at = format!("{} page {}", entry.path, i + 1);
        let content = match PageContent::read(src.pdf(), &page.contents, LIM) {
            Ok(content) => content,
            Err(fault) => {
                out.unread.push(format!("{at}: {fault}"));
                continue;
            }
        };
        let resources = page.resources.as_deref();
        let res = match res_cx(src.pdf(), resources, LIM) {
            Ok(res) => res,
            Err(e) => {
                out.unread
                    .push(format!("{at}: {}", ContentFault::Resources(e)));
                continue;
            }
        };
        let concat = content.concat();
        out.bytes += concat.bytes().len();
        if let Some(wrong) = untiled(concat.bytes(), &res) {
            out.unread.push(format!("{at}: tokens don't tile {wrong}"));
            continue;
        }
        // Each stream's own bytes, read apart from the concatenation.
        let own: Vec<Vec<u8>> = content
            .ids()
            .iter()
            .map(|&id| {
                let stream = src.pdf().get_object(id).unwrap().as_stream().unwrap();
                stream.decompressed_content().unwrap()
            })
            .collect();
        match splice(concat, &res, cs, &[]) {
            Ok(streams) => {
                for (n, (stream, own)) in streams.iter().zip(&own).enumerate() {
                    if **stream != **own || concat.stream(n) != own {
                        out.changed.push(format!("{at}, stream {n}"));
                    }
                }
            }
            Err(e) => {
                out.unread.push(format!("{at}: {e}"));
                continue;
            }
        }
        out.words += text_bag(src.pdf(), concat, resources, LIM, &fonts)
            .unwrap()
            .len();
        // The forms it draws, each once a file.
        let names = names_used(concat, &res, cs, &[]).unwrap();
        for name in names.of(ResKind::XObject) {
            let Some(id) = xobject(&src, resources, name) else {
                continue;
            };
            if !forms_seen.insert(id) {
                continue;
            }
            let form = src.pdf().get_object(id).ok().and_then(Object::as_stream);
            let Some(form) =
                form.filter(|f| f.dict.get(b"Subtype").and_then(Object::as_name) == Some(b"Form"))
            else {
                continue;
            };
            out.forms += 1;
            let at = format!("{at}, form {} {} R", id.number, id.generation);
            let Ok(data) = form.decompressed_content_with_limit(LIM.content.decompressed) else {
                out.unread.push(format!("{at}: won't decode"));
                continue;
            };
            let own = form.dict.get(b"Resources");
            let res = match res_cx(src.pdf(), own.or(resources), LIM) {
                Ok(res) => res,
                Err(e) => {
                    out.unread
                        .push(format!("{at}: {}", ContentFault::Resources(e)));
                    continue;
                }
            };
            if let Some(wrong) = untiled(&data, &res) {
                out.unread.push(format!("{at}: tokens don't tile {wrong}"));
                continue;
            }
            let one = Concat::new(&[&data]);
            match splice(&one, &res, cs, &[]) {
                Ok(streams) if *streams[0] == *data => {}
                Ok(_) => out.changed.push(at),
                Err(e) => out.unread.push(format!("{at}: {e}")),
            }
        }
        match PageBlocks::of(src.pdf(), &content, resources, LIM, &known) {
            Ok(blocks) => {
                for p in blocks.pagination.iter() {
                    *out.origins.entry((p.kind, p.origin)).or_default() += 1;
                    out.kinds_found.insert(p.kind);
                    if p.origin == PagOrigin::Acrobat {
                        out.acrobat_found.insert(p.kind);
                    }
                    let cuts = p.cuts(concat, &res, LIM).unwrap();
                    let made = splice(concat, &res, cs, &cuts).is_ok();
                    *out.splices.entry((p.origin, made)).or_default() += 1;
                    if p.origin == PagOrigin::Acrobat && !made {
                        out.acrobat_refused.push(format!("{at}: {:?}", p.kind));
                    }
                    let range = blocks.marked.iter().find(|m| m.span == p.span);
                    let inline = range.is_some_and(|m| m.list.is_some() && !m.named);
                    let made = match without_keys(concat, &res, cs, p.span, &[b"Type"]) {
                        Ok(_) => true,
                        Err(e) => {
                            if inline {
                                out.unrewritable.push(format!("{at}: {e}"));
                            }
                            false
                        }
                    };
                    *out.rewrites.entry((inline, made)).or_default() += 1;
                }
                // What a filter or a generator's `replaces` selects.
                let origins = BTreeSet::from(PagOrigin::REPLACED);
                for kinds in [
                    BTreeSet::from([PaginationKind::Header, PaginationKind::Footer]),
                    BTreeSet::from([PaginationKind::Watermark]),
                    BTreeSet::from([PaginationKind::Background]),
                ] {
                    let cuts: Vec<SpanAcross> = (blocks.selected(&kinds, &origins))
                        .flat_map(|p| p.cuts(concat, &res, LIM).unwrap())
                        .collect();
                    if let Err(e) = splice(concat, &res, cs, &cuts) {
                        out.unstrippable.push(format!("{at}, {kinds:?}: {e}"));
                    }
                }
                out.nested += usize::from(blocks.nested_in_forms);
                out.kept += blocks.marked.len();
                out.most_kept = out.most_kept.max(blocks.marked.len());
            }
            Err(fault) => out.unbalanced.push(format!("{at}: {fault}")),
        }
    }
    out
}

/// Every page of every form in the corpus, its content and the forms it
/// draws: read, its tokens tiling it, spliced with nothing taken out,
/// classified, and spliced without each pagination artifact.
#[test]
fn corpus_content_reads_splices_and_classifies() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let entries = corpus.forms().unwrap();
    let started = Instant::now();
    let outcomes = each_entry(&entries, |entry| read_file(&corpus, entry));
    let took = started.elapsed();
    let sum = |f: fn(&Outcome) -> usize| outcomes.iter().map(f).sum::<usize>();
    let all = |f: fn(&Outcome) -> &Vec<String>| outcomes.iter().flat_map(f).collect::<Vec<_>>();
    let (unread, unbalanced) = (all(|o| &o.unread), all(|o| &o.unbalanced));
    let changed = all(|o| &o.changed);
    let (refused, unstrippable) = (all(|o| &o.acrobat_refused), all(|o| &o.unstrippable));
    let unrewritable = all(|o| &o.unrewritable);
    let mut origins: BTreeMap<(PaginationKind, PagOrigin), usize> = BTreeMap::new();
    let mut splices: BTreeMap<(PagOrigin, bool), usize> = BTreeMap::new();
    let mut rewrites: BTreeMap<(bool, bool), usize> = BTreeMap::new();
    for o in &outcomes {
        for (key, n) in &o.origins {
            *origins.entry(*key).or_default() += n;
        }
        for (key, n) in &o.splices {
            *splices.entry(*key).or_default() += n;
        }
        for (key, n) in &o.rewrites {
            *rewrites.entry(*key).or_default() += n;
        }
    }
    eprintln!(
        "{} pages ({} MB of content) and {} forms in {} files, in {took:?}: \
         {} unread, {} with marked content unbalanced, {} nested in forms",
        sum(|o| o.pages),
        sum(|o| o.bytes) >> 20,
        sum(|o| o.forms),
        entries.len(),
        unread.len(),
        unbalanced.len(),
        sum(|o| o.nested),
    );
    eprintln!(
        "  {} words drawn; {} marked ranges kept, at most {} a page",
        sum(|o| o.words),
        sum(|o| o.kept),
        outcomes.iter().map(|o| o.most_kept).max().unwrap_or(0)
    );
    for line in unread.iter().chain(&unbalanced).take(20) {
        eprintln!("  {line}");
    }
    for ((kind, origin), n) in &origins {
        eprintln!("  {kind:?} by {origin:?}: {n}");
    }
    for origin in [PagOrigin::Sheaf, PagOrigin::Acrobat, PagOrigin::Authored] {
        let n = |made| splices.get(&(origin, made)).copied().unwrap_or(0);
        eprintln!(
            "  {origin:?}'s spliced out alone: {} made, {} refused",
            n(true),
            n(false)
        );
    }
    let n = |inline, made| rewrites.get(&(inline, made)).copied().unwrap_or(0);
    eprintln!(
        "  Lists written again without /Type: {} of {} in the content; {} of {} named",
        n(true, true),
        n(true, true) + n(true, false),
        n(false, true),
        n(false, true) + n(false, false),
    );
    assert!(unread.is_empty(), "{unread:#?}");
    assert!(unbalanced.is_empty(), "{unbalanced:#?}");
    // Word ends one page's first stream inside a footer's list, which no
    // rewrite of one stream reaches.
    let split = "forms/safedocs/0837070.pdf page 1: ";
    assert!(
        unrewritable.len() == 1 && unrewritable[0].starts_with(split),
        "{unrewritable:#?}"
    );
    assert!(changed.is_empty(), "{changed:#?}");
    // One page of Acrobat's watermark has the body's `q` and `BT` inside
    // its range, which no splice can take without the body.
    let opens_the_body = "forms/safedocs/0112530.pdf page 1";
    assert_eq!(refused, [&format!("{opens_the_body}: Watermark")]);
    assert_eq!(unstrippable.len(), 1, "{unstrippable:#?}");
    assert!(unstrippable[0].starts_with(&format!("{opens_the_body}, {{Watermark}}")));

    // Acrobat's files have Acrobat's artifacts of the kinds they say, but
    // where its forms are drawn unmarked, which is no artifact; Word's and
    // LibreOffice's have their authors' only.
    let (mut missed, mut unmarked) = (Vec::new(), 0);
    let mut office = (0, 0);
    for (entry, o) in entries.iter().zip(&outcomes) {
        let said: BTreeSet<_> = o.acrobat_said.difference(&o.acrobat_found).collect();
        if said.iter().any(|kind| o.kinds_found.contains(kind)) {
            missed.push(format!(
                "{}: says {:?}, found {:?}",
                entry.path, o.acrobat_said, o.origins
            ));
        } else if !said.is_empty() {
            unmarked += 1;
        }
        if o.office && o.acrobat_said.is_empty() && !o.origins.is_empty() {
            office.0 += 1;
            office.1 += o.origins.values().sum::<usize>();
            let not_authored: Vec<_> = o
                .origins
                .keys()
                .filter(|(_, origin)| *origin != PagOrigin::Authored)
                .collect();
            assert!(not_authored.is_empty(), "{}: {not_authored:?}", entry.path);
        }
    }
    let acrobat = outcomes
        .iter()
        .filter(|o| !o.acrobat_said.is_empty())
        .count();
    eprintln!(
        "{acrobat} files Acrobat drew in: {} as said, {unmarked} its forms unmarked; \
         {} Word or LibreOffice files mark {} artifacts, all their authors'",
        acrobat - missed.len() - unmarked,
        office.0,
        office.1
    );
    assert!(missed.is_empty(), "{missed:#?}");
}

/// Where [`glyphs_and_text_dump`] writes: a path. Unset, it reads nothing.
/// `just glyph-dump <file>` sets it.
const DUMP: &str = "SHEAF_GLYPH_DUMP";

/// A corpus path (`forms/irs/f1040.pdf`) whose records
/// [`glyphs_and_text_dump`] prints in full to stderr, reading that file
/// alone: to set two runs beside each other.
const DUMP_ONLY: &str = "SHEAF_GLYPH_DUMP_ONLY";

/// Every file the dump reads, with the set that picks it, each once, in
/// the order of their paths: every form, the pool's sample
/// (`bench/pool-sample.txt`), every scan, and the conformance suites'
/// files that claim PDF/UA. The files sheaf-render's interpreter tiers
/// read.
fn dump_picks(corpus: &Corpus) -> BTreeMap<String, &'static str> {
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

/// The dump's number for a font's kind. The dump numbers what it writes
/// itself, and writes each record from fields it names, never by a type's
/// `Debug`: so it says the same of the same font whatever type reads it.
fn kind_number(kind: FontKind) -> u64 {
    match kind {
        FontKind::Type1 => 0,
        FontKind::MMType1 => 1,
        FontKind::TrueType => 2,
        FontKind::Type3 => 3,
        FontKind::Type0(Descendant::CidFontType0) => 4,
        FontKind::Type0(Descendant::CidFontType2) => 5,
        // A kind the dump has no number for shows as one of its own.
        FontKind::Type0(_) => 254,
        _ => 255,
    }
}

/// The dump's number for what was read of a font's program.
fn program_number(program: FontProgram) -> u64 {
    match program {
        FontProgram::None => 0,
        FontProgram::Read => 1,
        FontProgram::Malformed => 2,
        FontProgram::Capped => 3,
        _ => 255,
    }
}

/// Records of the dump: each value written goes into the checksum, and,
/// where the records are printed in full, onto a line with its name.
struct Records {
    sum: sheaf_util::Checksum,
    full: Option<String>,
}

impl Records {
    fn new(full: bool) -> Records {
        Records {
            sum: sheaf_util::Checksum::new(),
            full: full.then(String::new),
        }
    }

    /// A record of kind `what` begins.
    fn begin(&mut self, what: &str) {
        self.sum.write(what.as_bytes());
        self.sum.write(&[0]);
        if let Some(full) = &mut self.full {
            full.push_str(what);
        }
    }

    fn number(&mut self, name: &str, v: u64) {
        self.sum.write(&v.to_le_bytes());
        if let Some(full) = &mut self.full {
            full.push_str(&format!(" {name}={v}"));
        }
    }

    /// A number that may not be there.
    fn maybe(&mut self, name: &str, v: Option<u64>) {
        self.sum.write(&[u8::from(v.is_some())]);
        self.sum.write(&v.unwrap_or(0).to_le_bytes());
        if let Some(full) = &mut self.full {
            match v {
                Some(v) => full.push_str(&format!(" {name}={v}")),
                None => full.push_str(&format!(" {name}=-")),
            }
        }
    }

    /// Numbers by their bits.
    fn bits(&mut self, name: &str, vs: &[f32]) {
        if let Some(full) = &mut self.full {
            full.push_str(&format!(" {name}="));
        }
        for (i, v) in vs.iter().enumerate() {
            self.sum.write(&v.to_bits().to_le_bytes());
            if let Some(full) = &mut self.full {
                let comma = if i == 0 { "" } else { "," };
                full.push_str(&format!("{comma}{:08x}({v})", v.to_bits()));
            }
        }
    }

    /// Bytes that may not be there, their length first.
    fn bytes(&mut self, name: &str, v: Option<&[u8]>) {
        self.sum.write(&[u8::from(v.is_some())]);
        let bytes = v.unwrap_or(&[]);
        self.sum.write(&(bytes.len() as u64).to_le_bytes());
        self.sum.write(bytes);
        if let Some(full) = &mut self.full {
            match v {
                Some(v) => {
                    let hex: String = v.iter().map(|b| format!("{b:02x}")).collect();
                    full.push_str(&format!(" {name}=<{hex}>"));
                }
                None => full.push_str(&format!(" {name}=-")),
            }
        }
    }

    /// A number that may be below 0.
    fn signed(&mut self, name: &str, v: i64) {
        self.sum.write(&v.to_le_bytes());
        if let Some(full) = &mut self.full {
            full.push_str(&format!(" {name}={v}"));
        }
    }

    /// Bytes too many to print, their length first: where the records
    /// are printed in full, their length and a checksum of them.
    fn blob(&mut self, name: &str, v: &[u8]) {
        self.sum.write(&(v.len() as u64).to_le_bytes());
        self.sum.write(v);
        if let Some(full) = &mut self.full {
            let sum = sheaf_util::checksum_hex(v);
            full.push_str(&format!(" {name}={}:{sum}", v.len()));
        }
    }

    /// Part of a page's content: where it starts and ends, each a stream
    /// and a byte in it.
    fn span(&mut self, name: &str, span: SpanAcross) {
        let (start, end) = (span.start, span.end);
        for v in [start.stream, start.at, end.stream, end.at] {
            self.sum.write(&u64::from(v).to_le_bytes());
        }
        if let Some(full) = &mut self.full {
            let (s, e) = (start, end);
            full.push_str(&format!(
                " {name}={}.{}..{}.{}",
                s.stream, s.at, e.stream, e.at
            ));
        }
    }

    /// An object as PDF writes it, which may not be there (0), or may be
    /// there and not be written (2).
    fn object(&mut self, name: &str, v: Option<&Object>) {
        let written = v.map(|v| {
            let mut out = Vec::new();
            crate::pdf::write::write(&mut out, v).map(|()| out)
        });
        match written {
            None => self.number(name, 0),
            Some(Ok(bytes)) => {
                self.number(name, 1);
                self.bytes(name, Some(&bytes));
            }
            Some(Err(_)) => self.number(name, 2),
        }
    }

    fn approx(&mut self, approx: FontApprox) {
        self.number("approx.codes", u64::from(approx.codes));
        self.number("approx.widths", u64::from(approx.widths));
        self.number("approx.heights", u64::from(approx.heights));
        self.number("approx.boxes", u64::from(approx.boxes));
    }

    /// The record ends.
    fn end(&mut self) {
        if let Some(full) = &mut self.full {
            full.push('\n');
        }
    }

    /// The checksum of every record, as the dump writes it; the records
    /// in full go to stderr under `title`.
    fn finish(self, title: &str) -> String {
        if let Some(full) = self.full {
            eprint!("== {title}\n{full}");
        }
        format!("{:032x}", self.sum.finish())
    }
}

/// A string shown's record: its font, what kind it is and what was read
/// of its program, whether there is one, what of it is guessed, whether
/// it's vertical, whether it's the stock one, and the string.
fn show_records(records: &mut Records, font: Option<&Font>, stock: bool, string: &[u8]) {
    records.begin("show");
    let id = font.and_then(Font::id);
    records.maybe("font", id.map(|id| u64::from(id.number)));
    records.maybe("generation", id.map(|id| u64::from(id.generation)));
    records.maybe("kind", font.map(|f| kind_number(f.kind())));
    records.maybe("program", font.map(|f| program_number(f.program())));
    records.number("has_font", u64::from(font.is_some()));
    records.approx(font.map(Font::approx).unwrap_or_default());
    records.number("vertical", u64::from(font.is_some_and(Font::is_vertical)));
    records.number("stock", u64::from(stock));
    records.bytes("string", Some(string));
    records.end();
}

/// A glyph's record: its code and CID, its quad's corners, its origin and
/// advance, what of it is guessed, whether it's invisible, and its text.
struct GlyphRecord<'g> {
    code: u32,
    cid: Option<u32>,
    quad: [f32; 8],
    origin: [f32; 2],
    advance: f32,
    approx: FontApprox,
    invisible: bool,
    text: Option<&'g str>,
}

fn glyph_records(records: &mut Records, g: GlyphRecord<'_>) {
    records.begin("glyph");
    records.number("code", u64::from(g.code));
    records.maybe("cid", g.cid.map(u64::from));
    records.bits("quad", &g.quad);
    records.bits("origin", &g.origin);
    records.bits("advance", &[g.advance]);
    records.approx(g.approx);
    records.number("invisible", u64::from(g.invisible));
    records.bytes("text", g.text.map(str::as_bytes));
    records.end();
}

/// The glyph dump's records of what `content`, drawn with `resources`
/// from `objects`' objects in `fonts`, shows, forms drawn: each string,
/// and each glyph, counted in `glyphs`.
fn shown<R: ReadObjects>(
    objects: &R,
    content: &Concat,
    resources: Option<&Object>,
    fonts: &Fonts,
    records: &mut Records,
    glyphs: &mut usize,
) -> Result<(), String> {
    let read = interpret(
        objects,
        content,
        resources,
        LIM,
        fonts,
        Opts::default(),
        |event| {
            let Kind::Text(show) = event.kind else {
                return ControlFlow::Continue(());
            };
            show_records(records, show.font, show.stock, show.string);
            for g in show.glyphs() {
                *glyphs += 1;
                let [a, b, c, d] = g.quad.0;
                let text = g.text();
                glyph_records(
                    records,
                    GlyphRecord {
                        code: g.code,
                        cid: g.cid,
                        quad: [a.x, a.y, b.x, b.y, c.x, c.y, d.x, d.y],
                        origin: [g.origin.x, g.origin.y],
                        advance: g.advance,
                        approx: g.approx,
                        invisible: g.invisible,
                        text: text.as_deref(),
                    },
                );
            }
            ControlFlow::Continue(())
        },
    );
    read.map(drop).map_err(|e| e.to_string())
}

/// `bag`'s words and hashes, each with its count, as records.
fn bag_records(records: &mut Records, bag: &TextBag) {
    for (word, n) in bag.words() {
        records.begin("word");
        records.bytes("text", Some(word.as_bytes()));
        records.number("n", n as u64);
        records.end();
    }
    for (hash, n) in bag.hashes() {
        records.begin("hash");
        records.bytes("of", Some(&hash.to_le_bytes()));
        records.number("n", n as u64);
        records.end();
    }
}

/// The dump's lines for the file at `path` of the corpus, one a page:
/// the file, the page, how many glyphs it lays out, the checksum of every
/// string it shows and each of its glyphs, the checksum of its text bag
/// and of the bags of the forms its content draws, and `ok` or why it
/// wasn't read. A file that doesn't open is one line, its page `-`.
fn dump_file(corpus: &Corpus, path: &str, full: bool) -> Vec<String> {
    let none = |why: String| vec![format!("{path}\t-\t0\t-\t-\t{why}")];
    let bytes = match std::fs::read(corpus.root.join(path)) {
        Ok(bytes) => bytes,
        Err(e) => return none(format!("unopened: {e}")),
    };
    let kind = BlobKind::file(Path::new(path), None);
    let src = match Store::new().open(bytes.into(), kind) {
        Ok(src) => src,
        Err(e) => return none(format!("unopened: {e:#}").replace(['\n', '\t'], " ")),
    };
    let fonts = Fonts::default();
    let for_text = Fonts::for_text();
    let mut lines = Vec::new();
    for (i, page) in src.pages().iter().enumerate() {
        let at = format!("{path} page {}", i + 1);
        let content = match PageContent::read(src.pdf(), &page.contents, LIM) {
            Ok(content) => content,
            Err(fault) => {
                lines.push(format!("{path}\t{}\t0\t-\t-\tunread: {fault}", i + 1));
                continue;
            }
        };
        let concat = content.concat();
        let resources = page.resources.as_deref();
        let mut why = Vec::new();

        // Every string shown, its forms' among them, and each glyph.
        let mut glyphs = 0usize;
        let mut records = Records::new(full);
        let read = shown(
            src.pdf(),
            concat,
            resources,
            &fonts,
            &mut records,
            &mut glyphs,
        );
        if let Err(e) = read {
            why.push(format!("interpreting: {e}"));
        }
        let shown = records.finish(&format!("{at}: shown"));

        // Its text bag, and those of the forms its content draws, each
        // judged on its own, as the removal check reads them.
        let bag = |content: &Concat, resources: Option<&Object>| {
            text_bag(src.pdf(), content, resources, LIM, &for_text).map_err(|e| e.to_string())
        };
        let mut records = Records::new(full);
        match bag(concat, resources) {
            Ok(bag) => bag_records(&mut records, &bag),
            Err(e) => why.push(format!("text bag: {e}")),
        }
        let family = LIM.content.family();
        let names = (res_cx(src.pdf(), resources, LIM).ok())
            .and_then(|res| names_used(concat, &res, &family, &[]).ok());
        let forms: BTreeSet<ObjectId> = (names.iter())
            .flat_map(|names| names.of(ResKind::XObject))
            .filter_map(|name| xobject(&src, resources, name))
            .collect();
        for id in forms {
            let form = src.pdf().get_object(id).ok().and_then(Object::as_stream);
            let Some(form) =
                form.filter(|f| f.dict.get(b"Subtype").and_then(Object::as_name) == Some(b"Form"))
            else {
                continue;
            };
            records.begin("form");
            records.number("id", u64::from(id.number));
            records.number("generation", u64::from(id.generation));
            let bag = (form.decompressed_content_with_limit(LIM.content.decompressed))
                .map_err(|e| e.to_string())
                .and_then(|data| {
                    let own = form.dict.get(b"Resources").or(resources);
                    bag(&Concat::new(&[&data]), own)
                });
            records.number("read", u64::from(bag.is_ok()));
            records.end();
            match bag {
                Ok(bag) => bag_records(&mut records, &bag),
                Err(e) => why.push(format!(
                    "form {} {} R's text bag: {e}",
                    id.number, id.generation
                )),
            }
        }
        let bags = records.finish(&format!("{at}: text bags"));

        let why = match why.is_empty() {
            true => "ok".to_string(),
            false => why.join("; ").replace(['\n', '\t'], " "),
        };
        lines.push(format!(
            "{path}\t{}\t{glyphs}\t{shown}\t{bags}\t{why}",
            i + 1
        ));
    }
    lines
}

/// What every page of the corpus's files shows, to set beside itself
/// after a change to what reads fonts: a line a page ([`dump_file`]),
/// the files in the order of their paths, written to the path in
/// [`DUMP`]. Each page is interpreted with fonts whose programs are read
/// ([`Fonts::default`]), and its text bag read with those read for text
/// ([`Fonts::for_text`]). Two runs over the same code write the same
/// file. With [`DUMP_ONLY`] it reads the one file named, and prints each
/// record in full to stderr.
#[test]
#[ignore = "a dump of the corpus's glyphs and text, written to $SHEAF_GLYPH_DUMP"]
fn glyphs_and_text_dump() {
    let to = std::env::var_os(DUMP);
    let only = std::env::var(DUMP_ONLY).ok();
    if to.is_none() && only.is_none() {
        eprintln!("skipped: neither {DUMP} nor {DUMP_ONLY} is set");
        return;
    }
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let started = Instant::now();
    let full = only.is_some();
    let lines = each_dumped(
        &corpus,
        (only.as_deref(), DUMP_ONLY),
        |path| dump_file(&corpus, path, full),
        |path, said| format!("{path}\t-\t0\t-\t-\tpanicked: {said}"),
    );
    let glyphs: usize = (lines.iter())
        .filter_map(|line| line.split('\t').nth(2)?.parse::<usize>().ok())
        .sum();
    let not_ok = lines.iter().filter(|line| !line.ends_with("\tok")).count();
    eprintln!(
        "{} lines, {glyphs} glyphs, {not_ok} not ok, in {:.0} s",
        lines.len(),
        started.elapsed().as_secs_f64()
    );
    written(&lines, full, to);
}

/// The lines of every file the dumps read ([`dump_picks`]), or of the
/// one `only.0` names (`only.1` the variable that named it), each file
/// read by `read` on its own, in the order of their paths. A file whose
/// reading panics is the one line `panicked` makes of its path and what
/// the panic said.
pub(crate) fn each_dumped(
    corpus: &Corpus,
    only: (Option<&str>, &str),
    read: impl Fn(&str) -> Vec<String> + Sync,
    panicked: impl Fn(&str, String) -> String + Sync,
) -> Vec<String> {
    let mut picks = dump_picks(corpus);
    let mut sets: BTreeMap<&str, usize> = BTreeMap::new();
    for set in picks.values() {
        *sets.entry(set).or_default() += 1;
    }
    eprintln!("{} files: {sets:?}", picks.len());
    if let (Some(only), var) = only {
        picks.retain(|path, _| path == only);
        assert_eq!(picks.len(), 1, "{var} names no file the dump reads");
    }
    let entries: Vec<Entry> = (picks.keys())
        .map(|path| Entry {
            path: path.clone(),
            ..Entry::default()
        })
        .collect();
    let read = each_entry(&entries, |entry| {
        let read = || read(&entry.path);
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(read)).unwrap_or_else(|panic| {
            let said = (panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .or_else(|| panic.downcast_ref::<String>().cloned());
            let said = said.unwrap_or_else(|| "a panic".into());
            vec![panicked(&entry.path, said.replace(['\n', '\t'], " "))]
        })
    });
    read.into_iter().flatten().collect()
}

/// A dump's `lines`, printed to stderr where its records were (`full`),
/// and written to `to`, where it's set.
pub(crate) fn written(lines: &[String], full: bool, to: Option<std::ffi::OsString>) {
    if full {
        for line in lines {
            eprintln!("{line}");
        }
    }
    if let Some(to) = to {
        let mut text = lines.join("\n");
        text.push('\n');
        std::fs::write(&to, text).unwrap_or_else(|e| panic!("{}: {e}", to.display()));
    }
}

/// Where [`events_dump`] writes: a path. Unset, it reads nothing.
/// `just events-dump <file>` sets it.
const EVENTS: &str = "SHEAF_EVENTS_DUMP";

/// A corpus path whose records [`events_dump`] prints in full to stderr,
/// reading that file alone.
const EVENTS_ONLY: &str = "SHEAF_EVENTS_DUMP_ONLY";

/// The records of the events dump: each value written goes into the
/// checksum of every event, and, but in an operator's record and the
/// leading ([`EventRecords::undrawn`]), into the checksum of what's
/// drawn, which a page written again must keep, as the `interp` fuzz
/// target holds it; where the records are printed in full, onto a line
/// with its name. A show's string and its font, which the glyph dump
/// has, go into what's drawn alone ([`EventRecords::drawn_only`]).
struct EventRecords {
    all: sheaf_util::Checksum,
    drawn: sheaf_util::Checksum,
    /// Whether what's being written goes into the checksum of every event
    /// alone: an operator's record, or the leading.
    op: bool,
    full: Option<String>,
}

impl EventRecords {
    fn new(full: bool) -> EventRecords {
        EventRecords {
            all: sheaf_util::Checksum::new(),
            drawn: sheaf_util::Checksum::new(),
            op: false,
            full: full.then(String::new),
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        self.all.write(bytes);
        if !self.op {
            self.drawn.write(bytes);
        }
    }

    fn name(&mut self, name: &str, v: impl std::fmt::Display) {
        if let Some(full) = &mut self.full {
            full.push_str(&format!(" {name}={v}"));
        }
    }

    /// A record of kind `what` begins: an operator's where `op`.
    fn begin(&mut self, what: &str, op: bool) {
        self.op = op;
        self.write(what.as_bytes());
        self.write(&[0]);
        if let Some(full) = &mut self.full {
            full.push_str(what);
        }
    }

    fn number(&mut self, name: &str, v: u64) {
        self.write(&v.to_le_bytes());
        self.name(name, v);
    }

    /// A number that may not be there.
    fn maybe(&mut self, name: &str, v: Option<u64>) {
        self.write(&[u8::from(v.is_some())]);
        self.write(&v.unwrap_or(0).to_le_bytes());
        match v {
            Some(v) => self.name(name, v),
            None => self.name(name, "-"),
        }
    }

    /// An object's id that may not be there.
    fn id(&mut self, name: &str, id: Option<ObjectId>) {
        self.write(&[u8::from(id.is_some())]);
        let (n, g) = id.map_or((0, 0), |id| (id.number, id.generation));
        self.write(&n.to_le_bytes());
        self.write(&g.to_le_bytes());
        if self.full.is_some() {
            let v = id.map_or("-".into(), |_| format!("{n}.{g}"));
            self.name(name, v);
        }
    }

    /// Numbers by their bits, a 0 of either sign the same.
    fn reals(&mut self, name: &str, vs: &[f32]) {
        for v in vs {
            self.write(&(v + 0.0).to_bits().to_le_bytes());
        }
        if self.full.is_some() {
            let vs: Vec<String> = (vs.iter())
                .map(|v| format!("{:08x}({v})", (v + 0.0).to_bits()))
                .collect();
            self.name(name, vs.join(","));
        }
    }

    /// Numbers that may not be there.
    fn maybe_reals(&mut self, name: &str, vs: Option<&[f32]>) {
        self.write(&[u8::from(vs.is_some())]);
        match vs {
            Some(vs) => self.reals(name, vs),
            None => self.name(name, "-"),
        }
    }

    /// Bytes that may not be there, their length first.
    fn bytes(&mut self, name: &str, v: Option<&[u8]>) {
        self.write(&[u8::from(v.is_some())]);
        let bytes = v.unwrap_or(&[]);
        self.write(&(bytes.len() as u64).to_le_bytes());
        self.write(bytes);
        if self.full.is_some() {
            self.name(name, v.map_or("-".into(), |v| format!("<{}>", hex(v))));
        }
    }

    /// What `write` writes goes into the checksum of every event alone:
    /// state the page written again needn't keep, nothing it draws
    /// reading it.
    fn undrawn(&mut self, write: impl FnOnce(&mut EventRecords)) {
        let op = std::mem::replace(&mut self.op, true);
        write(self);
        self.op = op;
    }

    /// What goes into the checksum of what's drawn alone: a show's string
    /// and its font, which the glyph dump has.
    fn drawn_only(&mut self, string: &[u8], font: Option<ObjectId>) {
        let (n, g) = font.map_or((0, 0), |id| (id.number, id.generation));
        for bytes in [&(string.len() as u64).to_le_bytes(), string] {
            self.drawn.write(bytes);
        }
        self.drawn.write(&[u8::from(font.is_some())]);
        self.drawn.write(&n.to_le_bytes());
        self.drawn.write(&g.to_le_bytes());
        if self.full.is_some() {
            self.name("string", format!("<{}>", hex(string)));
            self.name("font", font.map_or("-".into(), |_| format!("{n}.{g}")));
        }
    }

    /// The record ends.
    fn end(&mut self) {
        if let Some(full) = &mut self.full {
            full.push('\n');
        }
    }

    /// The checksums of every record and of what's drawn; the records in
    /// full go to stderr under `title`.
    fn finish(self, title: &str) -> (u128, u128) {
        if let Some(full) = self.full {
            eprint!("== {title}\n{full}");
        }
        (self.all.finish(), self.drawn.finish())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn matrix(m: Matrix) -> [f32; 6] {
    [m.a, m.b, m.c, m.d, m.e, m.f]
}

fn rect(r: Rect) -> [f32; 4] {
    [r.x0, r.y0, r.x1, r.y1]
}

/// The dump's numbers for what the events carry: each numbered by the
/// dump, never by a type's `Debug`, so the same reading is the same
/// record whatever types hold it.
fn paint_number(paint: Paint) -> u64 {
    match paint {
        Paint::Fill => 0,
        Paint::FillEvenOdd => 1,
        Paint::Stroke => 2,
        Paint::CloseStroke => 3,
        Paint::FillStroke => 4,
        Paint::FillStrokeEvenOdd => 5,
        Paint::CloseFillStroke => 6,
        Paint::CloseFillStrokeEvenOdd => 7,
        Paint::None => 8,
    }
}

fn rule_number(rule: FillRule) -> u64 {
    match rule {
        FillRule::NonZero => 0,
        FillRule::EvenOdd => 1,
    }
}

fn cap_number(cap: LineCap) -> u64 {
    match cap {
        LineCap::Butt => 0,
        LineCap::Round => 1,
        LineCap::Square => 2,
    }
}

fn join_number(join: LineJoin) -> u64 {
    match join {
        LineJoin::Miter => 0,
        LineJoin::Round => 1,
        LineJoin::Bevel => 2,
    }
}

fn mode_number(mode: TextMode) -> u64 {
    match mode {
        TextMode::Fill => 0,
        TextMode::Stroke => 1,
        TextMode::FillStroke => 2,
        TextMode::Invisible => 3,
        TextMode::FillClip => 4,
        TextMode::StrokeClip => 5,
        TextMode::FillStrokeClip => 6,
        TextMode::Clip => 7,
    }
}

/// A mark's record: its tag, whether it has a property list and how it's
/// written, its MCID, `/ActualText` and optional content.
fn mark_records(r: &mut EventRecords, mark: &Mark) {
    r.bytes("tag", Some(&mark.tag));
    let props = match &mark.props {
        None => 0,
        Some(MarkProps::Inline(_)) => 1,
        Some(MarkProps::Named(_, None)) => 2,
        Some(MarkProps::Named(_, Some(_))) => 3,
    };
    r.number("props", props);
    r.maybe("mcid", mark.mcid.map(u64::from));
    r.bytes(
        "actual_text",
        mark.actual_text.as_deref().map(str::as_bytes),
    );
    r.id("oc", mark.oc);
}

/// A color space's record: which it is, and its name where it's a
/// resource's.
fn space_records(r: &mut EventRecords, name: &'static str, space: &ActiveColorSpace) {
    let number = match space {
        ActiveColorSpace::DeviceGray => 0,
        ActiveColorSpace::DeviceRgb => 1,
        ActiveColorSpace::DeviceCmyk => 2,
        ActiveColorSpace::Pattern => 3,
        ActiveColorSpace::Named(_) => 4,
        _ => 5,
    };
    r.number(name, number);
    r.bytes(name, space.resource_name());
}

fn blend_number(blend: Blend) -> u64 {
    match blend {
        Blend::Normal => 0,
        Blend::Multiply => 1,
        Blend::Screen => 2,
        Blend::Overlay => 3,
        Blend::Darken => 4,
        Blend::Lighten => 5,
        Blend::ColorDodge => 6,
        Blend::ColorBurn => 7,
        Blend::HardLight => 8,
        Blend::SoftLight => 9,
        Blend::Difference => 10,
        Blend::Exclusion => 11,
        Blend::Hue => 12,
        Blend::Saturation => 13,
        Blend::Color => 14,
        Blend::Luminosity => 15,
    }
}

/// The state an event is in, every field of pdf-gfx's [`State`] but one:
/// the CTM; the line's width, cap, join, miter limit and dash; the
/// rendering intent and the flatness; each color's space, components and
/// pattern; each alpha, whether it's a shape's, and the text knockout;
/// the blend mode and the soft mask; the overprints and their mode; the
/// clip's bounds; and the text state: the font's being the stock one, its
/// size, the spacing, scale, leading, rise and mode, the text matrix,
/// where the line starts, the pen, and whether a width before it was
/// guessed. All but the leading are what's drawn. The one left out is
/// which font is picked, a place among the run's fonts in the order it
/// picked them, which says nothing of the font: the glyph dump has each
/// font's glyphs.
fn state_records(r: &mut EventRecords, state: &State) {
    r.reals("ctm", &matrix(state.ctm));
    r.reals("line_width", &[state.line_width]);
    r.number("cap", cap_number(state.line_cap));
    r.number("join", join_number(state.line_join));
    r.reals("miter", &[state.miter_limit]);
    r.number("dash_len", state.dash.array.len() as u64);
    r.reals("dash", &state.dash.array);
    r.reals("dash_phase", &[state.dash.phase]);
    let intent = match &state.rendering_intent {
        RenderingIntent::AbsoluteColorimetric => (0, None),
        RenderingIntent::RelativeColorimetric => (1, None),
        RenderingIntent::Saturation => (2, None),
        RenderingIntent::Perceptual => (3, None),
        RenderingIntent::Other(name) => (4, Some(&name[..])),
        _ => (5, None),
    };
    r.number("intent", intent.0);
    r.bytes("intent", intent.1);
    r.reals("flatness", &[state.flatness]);
    space_records(r, "stroke_space", &state.stroke_color_space);
    space_records(r, "fill_space", &state.fill_color_space);
    for (name, color) in [
        ("stroke_color", &state.stroke_color),
        ("fill_color", &state.fill_color),
    ] {
        r.number("components", color.as_slice().len() as u64);
        r.reals(name, color.as_slice());
    }
    r.bytes("stroke_pattern", state.stroke_pattern.as_deref());
    r.bytes("fill_pattern", state.fill_pattern.as_deref());
    r.reals("alpha", &[state.stroke_alpha, state.fill_alpha]);
    r.number("alpha_is_shape", u64::from(state.alpha_is_shape));
    r.number("text_knockout", u64::from(state.text_knockout));
    let blend = match &state.blend_mode {
        BlendState::Mode(blend) => (blend_number(*blend), None),
        BlendState::Compatible => (16, None),
        BlendState::Other(name) => (17, Some(&name[..])),
        _ => (18, None),
    };
    r.number("blend", blend.0);
    r.bytes("blend", blend.1);
    match state.soft_mask {
        SoftMask::None => r.number("soft_mask", 0),
        SoftMask::Mask { kind, group } => {
            r.number("soft_mask", 1);
            let kind = kind.map(|kind| match kind {
                SoftMaskKind::Alpha => 0,
                SoftMaskKind::Luminosity => 1,
                _ => 2,
            });
            r.maybe("soft_mask_kind", kind);
            r.id("soft_mask_group", group);
        }
        _ => r.number("soft_mask", 2),
    }
    r.number("stroke_overprint", u64::from(state.stroke_overprint));
    r.number("fill_overprint", u64::from(state.fill_overprint));
    let mode = u32::from_le_bytes(state.overprint_mode.to_le_bytes());
    r.number("overprint_mode", u64::from(mode));
    r.maybe_reals(
        "clip",
        state.clip_bounds().map(rect).as_ref().map(|r| &r[..]),
    );
    let text = &state.text;
    r.number("stock", u64::from(text.stock));
    r.reals("size", &[text.size]);
    r.reals("spacing", &[text.char_space, text.word_space]);
    r.reals("scale", &[text.scale]);
    // `TD` sets the leading, and the encoding writes it, and every
    // operator that reads the leading, as ones that don't.
    r.undrawn(|r| r.reals("leading", &[text.leading]));
    r.reals("rise", &[text.rise]);
    r.number("mode", mode_number(text.mode));
    r.reals("matrix", &matrix(text.matrix));
    r.reals("line", &[text.line.x, text.line.y]);
    r.reals("pen", &[text.pen.x, text.pen.y]);
    r.number("guessed", u64::from(text.guessed));
}

/// `event`'s record, by fields it names: what it is, the state it's in,
/// and the marked ranges it's in, innermost first. An operator's is its
/// name and where it is, from its first operand and itself; a show's its
/// own fields, its string and glyphs being the glyph dump's. A kind of
/// event the dump has no record for is one of its own (`other`).
fn event_records(r: &mut EventRecords, event: &Event<'_>) {
    match &event.kind {
        Kind::Op => {
            r.begin("op", true);
            r.bytes("name", Some(event.at.name()));
            let range = event.at.range();
            r.number("from", range.start as u64);
            let op = event.at.op_range();
            r.number("start", op.start as u64);
            r.number("end", op.end as u64);
        }
        Kind::Text(show) => {
            r.begin("text", false);
            r.reals("size", &[show.size]);
            r.number("mode", mode_number(show.mode));
            r.reals("start", &matrix(show.start));
            r.reals("offset", &[show.offset]);
            r.number("stock", u64::from(show.stock));
            r.drawn_only(show.string, show.font.and_then(Font::id));
        }
        Kind::FormBegin(form) | Kind::FormEnd(form) => {
            let begins = matches!(event.kind, Kind::FormBegin(_));
            r.begin(if begins { "form-begin" } else { "form-end" }, false);
            r.id("form", Some(form.form));
            r.reals("matrix", &matrix(form.matrix));
            r.maybe_reals("bbox", form.bbox.map(rect).as_ref().map(|r| &r[..]));
            r.id("oc", form.oc);
        }
        Kind::FormRefused { form, why } => {
            r.begin("form-refused", false);
            r.id("form", Some(*form));
            let why = match why {
                Refused::Cycle => 0,
                Refused::Depth => 1,
                _ => 255,
            };
            r.number("why", why);
        }
        Kind::Path(path) => {
            r.begin("path", false);
            r.number("segments", path.segments.len() as u64);
            for seg in path.segments {
                match *seg {
                    Seg::Move(p) => r.reals("m", &[p.x, p.y]),
                    Seg::Line(p) => r.reals("l", &[p.x, p.y]),
                    Seg::Curve(a, b, p) => r.reals("c", &[a.x, a.y, b.x, b.y, p.x, p.y]),
                    Seg::Close => r.number("h", 0),
                }
            }
            r.number("paint", paint_number(path.paint));
            r.maybe("clip", path.clip.map(rule_number));
            r.reals("bounds", &rect(path.bounds));
        }
        Kind::Image(image) => {
            r.begin("image", false);
            let [a, b, c, d] = image.quad.0;
            r.reals("quad", &[a.x, a.y, b.x, b.y, c.x, c.y, d.x, d.y]);
            let (inline, xobject) = match image.of {
                ImageOf::XObject(id) => (false, Some(id)),
                ImageOf::Inline { .. } => (true, None),
                _ => (true, Some(ObjectId::new(0, u16::MAX))),
            };
            r.number("inline", u64::from(inline));
            r.id("xobject", xobject);
            r.number("stencil", u64::from(image.stencil));
            r.id("smask", image.smask);
            r.id("mask", image.mask);
            r.id("oc", image.oc);
        }
        Kind::Shading(shading) => {
            r.begin("shading", false);
            r.id("shading", shading.shading);
            r.maybe_reals("bounds", shading.bounds.map(rect).as_ref().map(|r| &r[..]));
        }
        Kind::Ignored(id) => {
            r.begin("ignored", false);
            r.id("xobject", Some(*id));
        }
        Kind::MarkedBegin(mark) | Kind::MarkedEnd(mark) | Kind::MarkedPoint(mark) => {
            let what = match event.kind {
                Kind::MarkedBegin(_) => "marked-begin",
                Kind::MarkedEnd(_) => "marked-end",
                _ => "marked-point",
            };
            r.begin(what, false);
            mark_records(r, mark);
        }
        _ => r.begin("other", false),
    }
    state_records(r, event.state);
    for mark in event.marks.iter() {
        r.begin("in", r.op);
        mark_records(r, mark);
    }
    r.end();
}

/// The class of what stopped an interpretation, never its wording: a
/// damaged object where forms are drawn, sheaf's rule, is `damaged`, and
/// pdf-gfx's errors are named as it names them: content whose tokens stop
/// or is past one of content's caps is `content`, a form whose stream
/// doesn't decode `form-stream`, a cap `limit:<cap>`, geometry past an
/// `f32` `not-finite`, an operator that can't be written again
/// `unwritable:<why>`, an object that doesn't read for a reason that
/// isn't the file's `unreadable:<why>`, a page stream that wasn't decoded
/// `page-stream`, an inline image sized under a cap `capped-inline`.
/// Anything else is `other`, a finding.
fn interp_class(e: &Error) -> String {
    let e = match e {
        Error::Damaged(_) => return "damaged".into(),
        Error::Gfx(e) => e,
    };
    match e {
        GfxError::Content { .. } => "content".into(),
        GfxError::FormStream { .. } => "form-stream".into(),
        GfxError::Limit(cap) => format!("limit:{}", cap_name(*cap)),
        GfxError::NotFinite => "not-finite".into(),
        GfxError::Unwritable { why, .. } => format!("unwritable:{}", unwritable_name(*why)),
        GfxError::Unreadable(unreadable) => format!("unreadable:{}", why_name(unreadable.why)),
        GfxError::PageStream(_) => "page-stream".into(),
        GfxError::CappedInlineImage { .. } => "capped-inline".into(),
        _ => "other".into(),
    }
}

fn cap_name(cap: Cap) -> &'static str {
    match cap {
        Cap::Operators => "operators",
        Cap::Glyphs => "glyphs",
        Cap::Interpreted => "interpreted",
        Cap::Segments => "segments",
        Cap::States => "states",
        Cap::Marked => "marked",
        Cap::Decompressed => "decompressed",
        _ => "other",
    }
}

fn unwritable_name(why: Unwritable) -> &'static str {
    match why {
        Unwritable::Line => "line",
        Unwritable::InlineLength => "inline-length",
        Unwritable::InlineStream => "inline-stream",
        Unwritable::LoneBi => "lone-bi",
        _ => "other",
    }
}

fn why_name(why: Why) -> &'static str {
    match why {
        Why::Damaged => "damaged",
        Why::Capped => "capped",
        Why::Locked => "locked",
        Why::Source => "source",
        Why::Limit => "limit",
        Why::Filter => "filter",
        Why::Detached => "detached",
        _ => "other",
    }
}

/// The class of why a page wasn't written again: the encoder's own
/// refusals as [`interp_class`] names them, and an interpretation in the
/// encoder that stopped where the page's didn't as `encode:<class>`.
fn encode_class(e: &Error) -> String {
    match e {
        Error::Gfx(
            GfxError::Unwritable { .. }
            | GfxError::PageStream(_)
            | GfxError::CappedInlineImage { .. },
        ) => interp_class(e),
        _ => format!("encode:{}", interp_class(e)),
    }
}

/// A summary's fields, in its order, tab-separated.
fn summary_columns(s: &Summary) -> String {
    let Summary {
        operators,
        glyphs,
        interpreted,
        decompressed,
        segments,
        uses,
        refused,
        deepest_form,
        deepest_q,
        deepest_marked,
        stopped,
        ..
    } = *s;
    let fields = [
        operators,
        glyphs,
        interpreted,
        decompressed,
        segments,
        uses,
        refused,
        deepest_form,
        deepest_q,
        deepest_marked,
        usize::from(stopped),
    ];
    fields.map(|n| n.to_string()).join("\t")
}

/// How many columns a [`Summary`] takes.
const SUMMARY_COLUMNS: usize = 11;

/// A source's objects with each form a page's content draws written again
/// in its place, its stream's data what [`encode`] wrote, unfiltered, as
/// [`Fixture::encoded`](super::fixture::Fixture::encoded) writes them.
struct Rewritten<'a, O> {
    old: &'a O,
    forms: HashMap<ObjectId, Object>,
}

impl<'a, O: ReadObjects> Rewritten<'a, O> {
    /// `old`'s objects with each of `forms` written again in its place:
    /// an encoding's forms, each its id and data.
    fn new(old: &'a O, forms: &[(ObjectId, Vec<u8>)]) -> Rewritten<'a, O> {
        let forms = (forms.iter())
            .filter_map(|(id, data)| {
                let mut form = ReadObjects::read(old, *id, Object::clone).ok()?;
                form.as_stream_mut()?.set_plain_content(data.clone());
                Some((*id, form))
            })
            .collect();
        Rewritten { old, forms }
    }
}

impl<O: ReadObjects> ReadObjects for Rewritten<'_, O> {
    fn object(&self, id: ObjectId) -> crate::pdf::Result<&Object> {
        match self.forms.get(&id) {
            Some(form) => Ok(form),
            None => ReadObjects::object(self.old, id),
        }
    }

    fn read<R>(&self, id: ObjectId, read: impl FnOnce(&Object) -> R) -> crate::pdf::Result<R> {
        match self.forms.get(&id) {
            Some(form) => Ok(read(form)),
            None => ReadObjects::read(self.old, id, read),
        }
    }

    fn peek(&self, id: ObjectId) -> crate::pdf::Result<Option<Cow<'_, Object>>> {
        match self.forms.get(&id) {
            Some(form) => Ok(Some(Cow::Borrowed(form))),
            None => ReadObjects::peek(self.old, id),
        }
    }
}

/// The events dump's line for a page, or a file, that wasn't interpreted:
/// `class` says why.
fn events_none(path: &str, page: &str, class: &str) -> String {
    let dashes = ["-"; SUMMARY_COLUMNS + 2].join("\t");
    format!("{path}\t{page}\t{dashes}\t{class}\t-")
}

/// `content`, drawn with `resources` from `objects`' objects, its forms
/// drawn and every event wanted, each event's record written to
/// `records` ([`event_records`]).
fn recorded<R: ReadObjects>(
    objects: &R,
    content: &Concat,
    resources: Option<&Object>,
    fonts: &Fonts,
    records: &mut EventRecords,
) -> Result<Summary, Error> {
    interpret(
        objects,
        content,
        resources,
        LIM,
        fonts,
        Opts::default(),
        |event| {
            event_records(records, &event);
            ControlFlow::Continue(())
        },
    )
}

/// Whether `encoded`, content of `objects` written again, interpreted
/// again with each form written again in its place ([`Rewritten`]),
/// draws what the content did, `drawn` being the checksum of what that
/// drew ([`EventRecords::finish`]): every event but its operators, to the
/// bit, as the `interp` fuzz target holds it. Its records in full go to
/// stderr under `title`.
fn drawn_again<R: ReadObjects>(
    objects: &R,
    encoded: &EncodedPage,
    resources: Option<&Object>,
    fonts: &Fonts,
    drawn: u128,
    (title, full): (&str, bool),
) -> bool {
    let again = Rewritten::new(objects, &encoded.forms);
    let mut records = EventRecords::new(full);
    let content = Concat::new(&[&encoded.content]);
    let read = recorded(&again, &content, resources, fonts, &mut records);
    let (_, redrawn) = records.finish(title);
    read.is_ok() && redrawn == drawn
}

/// The events dump's columns for one page's `content`, drawn with
/// `resources` from `objects`' objects in `fonts`, after its file and its
/// number: its interpretation's [`Summary`] field by field, the checksum
/// of every event ([`event_records`]), the checksum of the page written
/// again ([`encode`]: its content, then each form's id and bytes), `ok` or
/// the class of what failed, whether the page written again, interpreted
/// again, draws what it drew ([`drawn_again`]: `same`, `differs`, or `-`
/// where it isn't written again). Its records in full go to stderr under
/// `at`, where `full`.
fn page_events<R: ReadObjects>(
    objects: &R,
    content: &Concat,
    resources: Option<&Object>,
    fonts: &Fonts,
    (at, full): (&str, bool),
) -> String {
    let mut records = EventRecords::new(full);
    let read = recorded(objects, content, resources, fonts, &mut records);
    let (events, drawn) = records.finish(&format!("{at}: events"));
    let summary = match &read {
        Ok(summary) => summary_columns(summary),
        Err(_) => ["-"; SUMMARY_COLUMNS].join("\t"),
    };
    let (encoding, why, redrawn) = match read {
        Err(e) => ("-".to_string(), interp_class(&e), "-"),
        Ok(_) => match encode(objects, content, resources, LIM, fonts) {
            Err(e) => ("-".to_string(), encode_class(&e), "-"),
            Ok(encoded) => {
                let mut sum = sheaf_util::Checksum::new();
                sum.write(&(encoded.content.len() as u64).to_le_bytes());
                sum.write(&encoded.content);
                for (id, data) in &encoded.forms {
                    sum.write(&id.number.to_le_bytes());
                    sum.write(&id.generation.to_le_bytes());
                    sum.write(&(data.len() as u64).to_le_bytes());
                    sum.write(data);
                }
                let title = format!("{at}: written again");
                let same = drawn_again(objects, &encoded, resources, fonts, drawn, (&title, full));
                let same = if same { "same" } else { "differs" };
                (format!("{:032x}", sum.finish()), "ok".to_string(), same)
            }
        },
    };
    format!("{summary}\t{events:032x}\t{encoding}\t{why}\t{redrawn}")
}

/// The events dump's lines for the file at `path` of the corpus, one a
/// page: the file, the page, and its columns ([`page_events`]), each page
/// interpreted in fonts whose programs are read ([`Fonts::default`]). A
/// file that doesn't open is one line, its page `-`.
fn events_file(corpus: &Corpus, path: &str, full: bool) -> Vec<String> {
    let Ok(bytes) = std::fs::read(corpus.root.join(path)) else {
        return vec![events_none(path, "-", "unopened")];
    };
    let kind = BlobKind::file(Path::new(path), None);
    let Ok(src) = Store::new().open(bytes.into(), kind) else {
        return vec![events_none(path, "-", "unopened")];
    };
    let fonts = Fonts::default();
    let mut lines = Vec::new();
    for (i, page) in src.pages().iter().enumerate() {
        let at = format!("{path} page {}", i + 1);
        let n = (i + 1).to_string();
        let Ok(content) = PageContent::read(src.pdf(), &page.contents, LIM) else {
            lines.push(events_none(path, &n, "unread"));
            continue;
        };
        let resources = page.resources.as_deref();
        let concat = content.concat();
        let columns = page_events(src.pdf(), concat, resources, &fonts, (&at, full));
        lines.push(format!("{path}\t{n}\t{columns}"));
    }
    lines
}

/// What every page of the corpus's files does as it's interpreted, to set
/// beside itself after a change to the interpreter: a line a page
/// ([`events_file`]), the files in the order of their paths
/// ([`dump_picks`]: the glyph dump's), written to the path in [`EVENTS`].
/// Two runs over the same code write the same file. With [`EVENTS_ONLY`]
/// it reads the one file named, and prints each record in full to
/// stderr, those of each page written again after its own.
#[test]
#[ignore = "a dump of what the corpus's pages do as they're interpreted, written to $SHEAF_EVENTS_DUMP"]
fn events_dump() {
    let to = std::env::var_os(EVENTS);
    let only = std::env::var(EVENTS_ONLY).ok();
    if to.is_none() && only.is_none() {
        eprintln!("skipped: neither {EVENTS} nor {EVENTS_ONLY} is set");
        return;
    }
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let started = Instant::now();
    let full = only.is_some();
    let lines = each_dumped(
        &corpus,
        (only.as_deref(), EVENTS_ONLY),
        |path| events_file(&corpus, path, full),
        |path, said| {
            eprintln!("{path} panicked: {said}");
            events_none(path, "-", "panicked")
        },
    );
    let column =
        |line: &String, from_end: usize| line.rsplit('\t').nth(from_end).map(str::to_owned);
    let count = |from_end: usize, is: &str| {
        (lines.iter())
            .filter(|line| column(line, from_end).as_deref() == Some(is))
            .count()
    };
    let not_ok = lines.len() - count(1, "ok");
    eprintln!(
        "{} lines, {not_ok} not ok, {} written again and drawn the same, {} drawn otherwise, in {:.0} s",
        lines.len(),
        count(0, "same"),
        count(0, "differs"),
        started.elapsed().as_secs_f64()
    );
    written(&lines, full, to);
}

/// Every fixture page's events line says it was read, written again and,
/// written again, drawn the same, and says the same twice; and what's
/// written again is interpreted again, not taken on trust: an operator
/// more that draws nothing leaves it drawn the same, a line more drawn in
/// the content, or in a form written again, draws otherwise.
#[test]
fn each_fixture_s_events_are_drawn_the_same_written_again() {
    let fonts = Fonts::default();
    for (name, f) in fixture::every() {
        let content = f.content();
        let at = (name, false);
        let line = page_events(&f.pdf, content.concat(), f.resources(), &fonts, at);
        let last: Vec<&str> = line.rsplit('\t').take(2).collect();
        assert_eq!(last, ["same", "ok"], "{name}: {line}");
        let again = page_events(&f.pdf, content.concat(), f.resources(), &fonts, at);
        assert_eq!(again, line, "{name}");
    }
    let drawn_with = |f: &fixture::Fixture, more: &[u8], in_form: bool| {
        let (content, resources) = (f.content(), f.resources());
        let mut records = EventRecords::new(false);
        let read = recorded(&f.pdf, content.concat(), resources, &fonts, &mut records);
        read.expect("drawn");
        let (_, drawn) = records.finish("");
        let mut encoded = encode(&f.pdf, content.concat(), resources, LIM, &fonts);
        let encoded = encoded.as_mut().expect("written again");
        match in_form {
            false => encoded.content.extend_from_slice(more),
            true => encoded.forms[0].1.extend_from_slice(more),
        }
        drawn_again(&f.pdf, encoded, resources, &fonts, drawn, ("", false))
    };
    let (operators, forms) = (fixture::operators(), fixture::forms());
    assert!(drawn_with(&operators, b"", false));
    assert!(drawn_with(&operators, b"\nq Q", false));
    assert!(!drawn_with(&operators, b"\n0 0 m 1 1 l S", false));
    assert!(drawn_with(&forms, b"", true));
    assert!(!drawn_with(&forms, b"\n0 0 m 1 1 l S", true));
}

/// The events dump holds numbers by their bits, but for a 0's sign.
#[test]
fn the_events_dump_holds_numbers_by_their_bits_but_a_zero_s_sign() {
    let sum = |v: f32| {
        let mut records = EventRecords::new(false);
        records.reals("v", &[v]);
        records.finish("")
    };
    assert_eq!(sum(0.0), sum(-0.0));
    assert_ne!(sum(1.0), sum(f32::from_bits(1.0_f32.to_bits() + 1)));
}

/// Where [`content_dump`] writes: a path. Unset, it reads nothing.
/// `just content-dump <file>` sets it.
const CONTENT: &str = "SHEAF_CONTENT_DUMP";

/// A corpus path whose records [`content_dump`] prints in full to
/// stderr, reading that file alone.
const CONTENT_ONLY: &str = "SHEAF_CONTENT_DUMP_ONLY";

/// How many columns a line of the content dump has.
const CONTENT_COLUMNS: usize = 13;

/// The content dump's line for a page, a form or a file that wasn't read
/// as content: `class` says why, and the rest are `-`.
fn content_none(path: &str, page: &str, class: &str) -> String {
    let dashes = ["-"; CONTENT_COLUMNS - 3].join("\t");
    format!("{path}\t{page}\t{class}\t{dashes}")
}

/// The class of what stopped content's tokens, or its marked content,
/// never its wording.
fn syntax_class(kind: SyntaxErrorKind) -> &'static str {
    use SyntaxErrorKind as K;
    match kind {
        K::String => "string",
        K::HexString => "hex-string",
        K::Array => "array",
        K::Dict => "dict",
        K::InlineImage => "inline-image",
        K::Depth => "depth",
        K::Operands => "operands",
        K::InlineImageSize => "image-size",
        K::ImageEnds => "image-ends",
        K::Marked => "marked",
        K::Emc => "emc",
        K::MarkedSize => "marked-size",
        _ => "other",
    }
}

/// The class of why a page's content wasn't read, or its blocks weren't.
fn fault_class(fault: &ContentFault) -> &'static str {
    match fault {
        ContentFault::Stream { .. } => "unread:stream",
        ContentFault::TooLarge(_) => "unread:too-large",
        ContentFault::Syntax { error, .. } => syntax_class(error.kind),
        ContentFault::Resources(_) => "resources",
    }
}

/// A marked range's record, as pdf-content-streams found it: its tag, its
/// list (none, written in the content, or named, with the list the
/// resources have under the name), its span and its depth.
fn marked_records(r: &mut Records, m: &crate::pdf::content::Marked) {
    use crate::pdf::content::Props;
    r.begin("marked");
    r.bytes("tag", Some(&m.tag));
    match &m.props {
        None => r.number("props", 0),
        Some(Props::Inline(list)) => {
            r.number("props", 1);
            r.object("list", Some(&Object::from(list.clone())));
        }
        Some(Props::Named(name, list)) => {
            r.number("props", 2);
            r.bytes("named", Some(name));
            r.object("list", list.cloned().map(Object::from).as_ref());
        }
    }
    r.span("span", m.span);
    r.number("depth", u64::from(m.depth));
    r.end();
}

/// The record of a marked range a page's blocks keep: its tag, its list,
/// whether the list is named in the resources, and its span.
fn kept_records(r: &mut Records, m: &KeptRange) {
    r.begin("kept");
    r.bytes("tag", Some(&m.tag));
    r.number("named", u64::from(m.named));
    r.object("list", m.list.clone().map(Object::from).as_ref());
    r.span("span", m.span);
    r.end();
}

/// The dump's number for why a splice was refused.
fn refusal(e: &SpliceError) -> u64 {
    match e {
        SpliceError::Read(_) => 1,
        SpliceError::OutOfRange(_) => 2,
        SpliceError::Overlap(_) => 3,
        SpliceError::Unsafe(_) => 4,
        SpliceError::List(_) => 5,
        SpliceError::Capped(_) => 6,
        SpliceError::Reads { .. } => 7,
        _ => 255,
    }
}

/// What a splice made: refused, and why; made with no stream changed; or
/// made, and every stream's bytes.
fn spliced_records(r: &mut Records, made: Result<Option<Vec<Cow<'_, [u8]>>>, SpliceError>) {
    match made {
        Err(e) => {
            r.number("made", 0);
            r.number("refused", refusal(&e));
        }
        Ok(None) => {
            r.number("made", 1);
            r.number("changed", 0);
        }
        Ok(Some(streams)) => {
            r.number("made", 1);
            r.number("changed", 1);
            r.number("streams", streams.len() as u64);
            for stream in &streams {
                r.blob("stream", stream);
            }
        }
    }
}

/// What the content dump reads of one page's content, or one form's: its
/// columns 3 to 11, and what reading on needs of it.
struct ContentRead {
    columns: String,
    /// The color spaces it's read with, where they read.
    res: Option<ResCx>,
    /// The resource names it uses, where its tokens read.
    names: Option<NameSet>,
}

/// The content dump's columns 3 to 11 for `content`, drawn with
/// `resources` from `objects`' objects, as sheaf reads it, by
/// pdf-content-streams under [`ContentLimits::family`](crate::source::ContentLimits::family),
/// with the color spaces [`res_cx`] counts and the lists [`properties`]
/// reads:
/// 3. `ok`, or the class of what stopped its tokens ([`syntax_class`]), or
///    `resources` where its color spaces don't read, every column after
///    it `-` but 11;
/// 4. how many tokens; 5. a checksum of each token's kind and range, an
///    inline image's data's range, and where the tokens stopped;
/// 6. how many operators, an inline image one;
/// 7. its `q`/`Q` balance, `net,min`, or `-`;
/// 8. the resource names it uses, how many and a checksum of each
///    category and name, or `-`;
/// 9. its marked ranges, how many and a checksum of each ([`marked_records`]),
///    or `err:` and the class of what stopped them;
/// 10. the operands sheaf reads as more than bytes, how many and a
///     checksum of each: a name, decoded, and a dictionary, read as an
///     object and written, or that it isn't one;
/// 11. what the private names' walk hears of it ([`heard_column`]).
///
/// The color spaces it's read with ([`res_cx`]) have no column:
/// pdf-content-streams' `ResCx` doesn't lend its names and counts.
///
/// Its records in full go to stderr under `at`, where `full`.
fn content_columns<R: ReadObjects>(
    objects: &R,
    content: &Concat,
    resources: Option<&Object>,
    (at, full): (&str, bool),
) -> ContentRead {
    let lim = &LIM.content.family();
    let s = content.bytes();
    let Ok(res) = res_cx(objects, resources, LIM) else {
        let dashes = ["-"; 7].join("\t");
        let heard = heard_column(s, None, (at, full));
        return ContentRead {
            columns: format!("resources\t{dashes}\t{heard}"),
            res: None,
            names: None,
        };
    };
    let (mut read_tokens, mut operands) = (Records::new(full), Records::new(full));
    let (mut count, mut operators, mut read) = (0usize, 0usize, 0usize);
    let mut class = "ok";
    for token in tokens(s, &res, lim) {
        let (r, tok) = match token {
            Ok(token) => token,
            Err(e) => {
                class = syntax_class(e.kind);
                read_tokens.begin("stopped");
                read_tokens.number("at", e.at as u64);
                read_tokens.bytes("class", Some(class.as_bytes()));
                read_tokens.end();
                break;
            }
        };
        count += 1;
        read_tokens.begin(match tok {
            Tok::Op(_) => "op",
            Tok::Operand(_) => "operand",
            Tok::InlineImage(_) => "image",
        });
        read_tokens.number("start", r.start as u64);
        read_tokens.number("end", r.end as u64);
        match tok {
            Tok::Op(_) => operators += 1,
            Tok::InlineImage(data) => {
                operators += 1;
                read_tokens.number("data_start", data.start as u64);
                read_tokens.number("data_end", data.end as u64);
            }
            Tok::Operand(operand) => {
                if let Some(decoded) = name(operand) {
                    read += 1;
                    operands.begin("name");
                    operands.bytes("name", Some(&decoded));
                    operands.end();
                } else if operand.starts_with(b"<<") {
                    read += 1;
                    operands.begin("dict");
                    operands.object("object", object(operand, lim).as_ref());
                    operands.end();
                }
            }
        }
        read_tokens.end();
    }
    let read_tokens = read_tokens.finish(&format!("{at}: tokens"));
    let counted = match balance(content, &res, lim) {
        Ok(b) => format!("{},{}", b.net, b.min_prefix),
        Err(_) => "-".into(),
    };
    let names = names_used(content, &res, lim, &[]).ok();
    let used = match &names {
        Some(names) => {
            let mut r = Records::new(full);
            for (kind, name) in names.iter() {
                r.begin("name");
                r.bytes("kind", Some(kind.key()));
                r.bytes("name", Some(name));
                r.end();
            }
            format!("{}:{}", names.len(), r.finish(&format!("{at}: names")))
        }
        None => "-".into(),
    };
    let lists = properties(objects, resources);
    let ranges = match crate::pdf::content::marked(content, &lists, &res, lim) {
        Ok(found) => {
            let mut r = Records::new(full);
            for m in &found {
                marked_records(&mut r, m);
            }
            format!("{}:{}", found.len(), r.finish(&format!("{at}: marked")))
        }
        Err(e) => format!("err:{}", syntax_class(e.kind)),
    };
    let operands = format!("{read}:{}", operands.finish(&format!("{at}: operands")));
    let heard = heard_column(s, Some(&res), (at, full));
    ContentRead {
        columns: format!(
            "{class}\t{count}\t{read_tokens}\t{operators}\t{counted}\t{used}\t\
             {ranges}\t{operands}\t{heard}"
        ),
        res: Some(res),
        names,
    }
}

/// The content dump's column 11 for content `s`, read with `res` (None
/// where its resources don't read): what the private names' walk hears
/// of it ([`crate::private::heard`]), how many and a checksum of each
/// name, and `,unread` where it took the content to hold some it couldn't
/// read.
fn heard_column(s: &[u8], res: Option<&ResCx>, (at, full): (&str, bool)) -> String {
    let heard = crate::private::heard(s, res, LIM);
    let mut r = Records::new(full);
    for said in &heard {
        match said {
            Some(name) => {
                r.begin("name");
                r.bytes("name", Some(name));
            }
            None => r.begin("unread"),
        }
        r.end();
    }
    let unread = match heard.iter().any(Option::is_none) {
        true => ",unread",
        false => "",
    };
    format!(
        "{}:{}{unread}",
        heard.len(),
        r.finish(&format!("{at}: heard"))
    )
}

/// The content dump's number for who drew a pagination artifact.
fn origin_number(origin: PagOrigin) -> u64 {
    match origin {
        PagOrigin::Sheaf => 0,
        PagOrigin::Acrobat => 1,
        PagOrigin::Authored => 2,
    }
}

/// The content dump's column 12 for a page: its blocks, field by field
/// (each marked range kept, each pagination artifact's kind, origin,
/// generator and span, each of sheaf's blocks and where it is, the
/// streams that isolate, whether a stream is marked as sheaf's, the
/// balance, and whether a form drawn has an artifact in it), how many
/// ranges and artifacts and a checksum of them; or `err:` and the class
/// of the fault that stopped them.
fn blocks_column(blocks: &Result<PageBlocks, ContentFault>, (at, full): (&str, bool)) -> String {
    let b = match blocks {
        Ok(b) => b,
        Err(fault) => return format!("err:{}", fault_class(fault)),
    };
    let mut r = Records::new(full);
    for m in b.marked.iter() {
        kept_records(&mut r, m);
    }
    for p in b.pagination.iter() {
        r.begin("pagination");
        r.bytes("kind", Some(p.kind.name()));
        r.number("origin", origin_number(p.origin));
        r.maybe("generator", p.generator.map(|g| g.0));
        r.span("span", p.span);
        r.end();
    }
    for s in b.sheaf.iter() {
        r.begin("sheaf");
        let (block, id) = match s.block {
            SheafBlock::Gen(g) => (0, g.0),
            SheafBlock::Ocr(b) => (1, b.0),
            SheafBlock::Flat(b) => (2, b.0),
        };
        r.number("block", block);
        r.number("id", id);
        match s.at {
            BlockAt::Stream(id) => {
                r.number("at", 0);
                r.number("stream", u64::from(id.number));
                r.number("generation", u64::from(id.generation));
            }
            BlockAt::Marked(span) => {
                r.number("at", 1);
                r.span("span", span);
            }
        }
        r.end();
    }
    for id in b.isolation.iter() {
        r.begin("isolation");
        r.number("stream", u64::from(id.number));
        r.number("generation", u64::from(id.generation));
        r.end();
    }
    r.begin("page");
    r.number("sheaf_marked", u64::from(b.sheaf_marked));
    r.signed("net", b.balance.net.into());
    r.signed("min_prefix", b.balance.min_prefix.into());
    r.number("nested_in_forms", u64::from(b.nested_in_forms));
    r.end();
    let sum = r.finish(&format!("{at}: blocks"));
    format!("{},{}:{sum}", b.marked.len(), b.pagination.len())
}

/// The content dump's column 13 for a page, `content` read with `res`
/// from `objects`' objects: what's cut and written, as a strip, a filter
/// and a save make it. Whether a splice of nothing is made and gives
/// each stream's own bytes, read apart; for each pagination artifact of
/// `blocks`, its cuts ([`Pagination::cuts`]), what a splice of them makes
/// ([`spliced_records`]), and its list written again without `/Type`
/// ([`without_keys`]) and what that makes; and for each of the three
/// sets a strip selects ([`PageBlocks::selected`]), what a splice of
/// them together makes. How many artifacts' cuts are made of how many,
/// and a checksum of the lot.
fn written_column<R: ReadObjects>(
    objects: &R,
    content: &PageContent,
    res: &ResCx,
    blocks: Option<&PageBlocks>,
    (at, full): (&str, bool),
) -> String {
    let concat = content.concat();
    let cs = &LIM.content.family();
    let mut r = Records::new(full);
    r.begin("nothing");
    match splice(concat, res, cs, &[]) {
        Ok(streams) => {
            r.number("made", 1);
            r.number("streams", streams.len() as u64);
            for (n, (stream, &id)) in streams.iter().zip(content.ids()).enumerate() {
                let own = objects.read(id, |o| {
                    let stream = o.as_stream()?;
                    stream.decompressed_content().ok()
                });
                let own = own.ok().flatten();
                r.number("own", u64::from(own.as_deref() == Some(&**stream)));
                r.number(
                    "joined",
                    u64::from(own.as_deref() == Some(concat.stream(n))),
                );
            }
        }
        Err(e) => {
            r.number("made", 0);
            r.number("refused", refusal(&e));
        }
    }
    r.end();
    let (mut made, mut artifacts) = (0, 0);
    for p in blocks.iter().flat_map(|b| b.pagination.iter()) {
        artifacts += 1;
        r.begin("artifact");
        r.span("span", p.span);
        match p.cuts(concat, res, LIM) {
            Ok(cuts) => {
                r.number("cuts", cuts.len() as u64);
                for &cut in &cuts {
                    r.span("cut", cut);
                }
                let spliced = splice(concat, res, cs, &cuts);
                made += usize::from(spliced.is_ok());
                spliced_records(&mut r, spliced.map(Some));
            }
            Err(e) => r.bytes("stopped", Some(syntax_class(e.kind).as_bytes())),
        }
        match without_keys(concat, res, cs, p.span, &[b"Type"]) {
            Ok(Some(rewrite)) => {
                r.number("rewrite", 2);
                let nothing = Spliced::default();
                spliced_records(&mut r, nothing.streams(concat, res, cs, &[rewrite]));
            }
            Ok(None) => r.number("rewrite", 1),
            Err(e) => {
                r.number("rewrite", 0);
                r.number("refused", refusal(&e));
            }
        }
        r.end();
    }
    let origins = BTreeSet::from(PagOrigin::REPLACED);
    let sets = [
        BTreeSet::from([PaginationKind::Header, PaginationKind::Footer]),
        BTreeSet::from([PaginationKind::Watermark]),
        BTreeSet::from([PaginationKind::Background]),
    ];
    for (b, kinds) in blocks
        .iter()
        .flat_map(|b| sets.iter().map(move |kinds| (b, kinds)))
    {
        r.begin("selected");
        for kind in kinds {
            r.bytes("kind", Some(kind.name()));
        }
        let cuts: Result<Vec<Vec<SpanAcross>>, SyntaxError> = (b.selected(kinds, &origins))
            .map(|p| p.cuts(concat, res, LIM))
            .collect();
        match cuts {
            Ok(cuts) => {
                let cuts: Vec<SpanAcross> = cuts.into_iter().flatten().collect();
                r.number("cuts", cuts.len() as u64);
                spliced_records(&mut r, splice(concat, res, cs, &cuts).map(Some));
            }
            Err(e) => r.bytes("stopped", Some(syntax_class(e.kind).as_bytes())),
        }
        r.end();
    }
    format!("{made}/{artifacts}:{}", r.finish(&format!("{at}: written")))
}

/// A form XObject a page draws, as the content dump reads it: the page
/// label of its line, and its data and the resources it's read with, its
/// own or its page's, or the class of why its data wasn't read.
struct DrawnForm<'s> {
    label: String,
    read: Result<(Vec<u8>, Option<&'s Object>), &'static str>,
}

/// The form XObjects page `n` draws, by the XObject `names` it uses, that
/// no page before it drew (`seen`), as [`read_file`] finds them: in
/// `resources`, `/Subtype /Form`, each decoded under the content's cap.
fn forms_drawn<'s, 'n>(
    src: &'s crate::store::Src,
    resources: Option<&'s Object>,
    n: &str,
    names: impl Iterator<Item = &'n [u8]>,
    seen: &mut HashSet<ObjectId>,
) -> Vec<DrawnForm<'s>> {
    let mut drawn = Vec::new();
    for name in names {
        let Some(id) = xobject(src, resources, name) else {
            continue;
        };
        if !seen.insert(id) {
            continue;
        }
        let form = src.pdf().get_object(id).ok().and_then(Object::as_stream);
        let Some(form) =
            form.filter(|f| f.dict.get(b"Subtype").and_then(Object::as_name) == Some(b"Form"))
        else {
            continue;
        };
        let label = format!("{n} form {} {}", id.number, id.generation);
        let read = match form.decompressed_content_with_limit(LIM.content.decompressed) {
            Ok(data) => Ok((data, form.dict.get(b"Resources").or(resources))),
            Err(crate::pdf::Error::Limit(crate::pdf::Limit::Decompressed)) => {
                Err("unread:too-large")
            }
            Err(_) => Err("unread:stream"),
        };
        drawn.push(DrawnForm { label, read });
    }
    drawn
}

/// The content dump's lines for a file of `bytes`, at `path`: a line a
/// page, `{path}\t{page}\t` and its columns 3 to 11 ([`content_columns`]),
/// 12 ([`blocks_column`]) and 13 ([`written_column`], `-` where its
/// resources don't read); and after it a
/// line for each form XObject the page draws that no page before it
/// drew ([`forms_drawn`]), its page `{page} form {n} {generation}`, its
/// columns 3 to 11 read with its own resources or its page's, and 12 and
/// 13 `-`. A page, or a form, that isn't read is [`content_none`]'s line,
/// its class `unread:stream` or `unread:too-large`; a file that doesn't
/// open is one, its page `-` and its class `unopened`.
fn content_lines(path: &str, bytes: Vec<u8>, full: bool) -> Vec<String> {
    let kind = BlobKind::file(Path::new(path), None);
    let Ok(src) = Store::new().open(bytes.into(), kind) else {
        return vec![content_none(path, "-", "unopened")];
    };
    let known = DrawnForms::default();
    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    for (i, page) in src.pages().iter().enumerate() {
        let n = (i + 1).to_string();
        let at = format!("{path} page {n}");
        let content = match PageContent::read(src.pdf(), &page.contents, LIM) {
            Ok(content) => content,
            Err(fault) => {
                lines.push(content_none(path, &n, fault_class(&fault)));
                continue;
            }
        };
        let resources = page.resources.as_deref();
        let concat = content.concat();
        let read = content_columns(src.pdf(), concat, resources, (&at, full));
        let blocks = PageBlocks::of(src.pdf(), &content, resources, LIM, &known);
        let kept = blocks_column(&blocks, (&at, full));
        let written = match &read.res {
            Some(res) => {
                written_column(src.pdf(), &content, res, blocks.as_ref().ok(), (&at, full))
            }
            None => "-".into(),
        };
        lines.push(format!("{path}\t{n}\t{}\t{kept}\t{written}", read.columns));
        let names = read
            .names
            .iter()
            .flat_map(|names| names.of(ResKind::XObject));
        for form in forms_drawn(&src, resources, &n, names, &mut seen) {
            let label = form.label;
            let (data, own) = match form.read {
                Ok(read) => read,
                Err(class) => {
                    lines.push(content_none(path, &label, class));
                    continue;
                }
            };
            let at = format!("{path} page {label}");
            let one = Concat::new(&[&data]);
            let read = content_columns(src.pdf(), &one, own, (&at, full));
            lines.push(format!("{path}\t{label}\t{}\t-\t-", read.columns));
        }
    }
    lines
}

/// The content dump's lines for the file at `path` of the corpus
/// ([`content_lines`]).
fn content_file(corpus: &Corpus, path: &str, full: bool) -> Vec<String> {
    match std::fs::read(corpus.root.join(path)) {
        Ok(bytes) => content_lines(path, bytes, full),
        Err(_) => vec![content_none(path, "-", "unopened")],
    }
}

/// What every page of the corpus's files reads as content, and every
/// form XObject a page draws, once a file, to set beside itself after a
/// change to what reads content: a line each ([`content_lines`]), numbers
/// and checksums only, the files in the order of their paths ([`dump_picks`]: the
/// glyph dump's), written to the path in [`CONTENT`]. Two runs over the
/// same code write the same file. With [`CONTENT_ONLY`] it reads the one
/// file named, and prints each record in full to stderr.
#[test]
#[ignore = "a dump of what the corpus's content reads as, written to $SHEAF_CONTENT_DUMP"]
fn content_dump() {
    let to = std::env::var_os(CONTENT);
    let only = std::env::var(CONTENT_ONLY).ok();
    if to.is_none() && only.is_none() {
        eprintln!("skipped: neither {CONTENT} nor {CONTENT_ONLY} is set");
        return;
    }
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let started = Instant::now();
    let full = only.is_some();
    let lines = each_dumped(
        &corpus,
        (only.as_deref(), CONTENT_ONLY),
        |path| content_file(&corpus, path, full),
        |path, said| {
            eprintln!("{path} panicked: {said}");
            content_none(path, "-", "panicked")
        },
    );
    let column = |line: &String, n: usize| line.split('\t').nth(n).map(str::to_owned);
    let forms = (lines.iter())
        .filter(|line| column(line, 1).is_some_and(|page| page.contains(" form ")))
        .count();
    let not_ok = (lines.iter())
        .filter(|line| column(line, 2).as_deref() != Some("ok"))
        .count();
    eprintln!(
        "{} lines, {} pages and {forms} forms, {not_ok} not ok, in {:.0} s",
        lines.len(),
        lines.len() - forms,
        started.elapsed().as_secs_f64()
    );
    written(&lines, full, to);
}

/// A file of one page drawing `content`, with resources that name a
/// color space of three components (`/CS3`), a form (`/X0`) that draws a
/// marked range of its own, and a footer's property list (`/P0`).
fn content_fixture(content: &[u8]) -> Vec<u8> {
    use crate::pdf::{Document, Stream, dictionary};
    let mut pdf = Document::with_version("1.7");
    let form = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => Object::name("XObject"),
            "Subtype" => Object::name("Form"),
            "BBox" => vec![0.into(), 0.into(), 9.into(), 9.into()],
        },
        b"/Span <</MCID 0>> BDC 0 0 1 1 re f EMC".to_vec(),
    ));
    let content = pdf.add_object(Stream::new(dictionary! {}, content.to_vec()));
    let footer = dictionary! {
        "Type" => Object::name("Pagination"),
        "Subtype" => Object::name("Footer"),
    };
    let pages = pdf.new_object_id();
    let page = pdf.add_object(dictionary! {
        "Type" => Object::name("Page"),
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 99.into(), 99.into()],
        "Contents" => content,
        "Resources" => dictionary! {
            "ColorSpace" => dictionary! { "CS3" => Object::name("DeviceRGB") },
            "XObject" => dictionary! { "X0" => form },
            "Properties" => dictionary! { "P0" => footer },
        },
    });
    pdf.objects.insert(
        pages,
        Object::from(dictionary! {
            "Type" => Object::name("Pages"),
            "Kids" => vec![page.into()],
            "Count" => 1,
        }),
    );
    let catalog = pdf.add_object(dictionary! {
        "Type" => Object::name("Catalog"),
        "Pages" => pages,
    });
    pdf.trailer.set("Root", catalog);
    let mut out = Vec::new();
    pdf.save_to(&mut out).expect("writing the fixture");
    out
}

/// The first page [`the_content_dump_says_what_content_reads_as`] reads:
/// a header's list written in the content, a footer's named, an inline
/// image sized by a named color space, and a form drawn.
const DUMP_PAGE: &[u8] =
    b"q /Artifact <</Type /Pagination /Subtype /Header>> BDC Q q 0 g 0 0 9 9 re f Q EMC\n\
    /Artifact /P0 BDC BT /F1 9 Tf (f) Tj ET EMC\n\
    q BI /W 2 /H 1 /BPC 8 /CS /CS3 ID abcdef EI Q /X0 Do";

/// The second: tokens that stop at a string that doesn't end.
const DUMP_UNENDED: &[u8] = b"BT /SHEF_Gen 9 Tf (never ends";

/// The content dump's columns on two pages, each read the same twice and
/// the same where its records are printed in full: one with two
/// pagination artifacts, a header's list written in the content and a
/// footer's named, an inline image sized by a named color space, and a
/// form drawn, which has a line of its own; and one whose tokens stop at
/// a string that doesn't end, which the private names' walk hears as
/// unread, since it says `/SHEF_`.
#[test]
fn the_content_dump_says_what_content_reads_as() {
    let lines = |content: &[u8]| {
        let file = content_fixture(content);
        let lines = content_lines("fixture.pdf", file.clone(), false);
        assert_eq!(content_lines("fixture.pdf", file.clone(), false), lines);
        assert_eq!(content_lines("fixture.pdf", file, true), lines);
        lines
    };
    // Each column, a checksum's place written `#` where it's one.
    let columns = |line: &str| -> Vec<String> {
        let line: Vec<&str> = line.split('\t').collect();
        assert_eq!(line.len(), CONTENT_COLUMNS, "{line:?}");
        let sum = |s: &str| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit());
        (line.iter())
            .map(|column| match column.split_once(':') {
                Some((count, rest)) if sum(rest.trim_end_matches(",unread")) => {
                    let unread = rest.ends_with(",unread");
                    format!("{count}:#{}", if unread { ",unread" } else { "" })
                }
                _ if sum(column) => "#".into(),
                _ => column.to_string(),
            })
            .collect()
    };
    let read = lines(DUMP_PAGE);
    assert_eq!(read.len(), 2, "{read:#?}");
    // 32 tokens, 19 operators, the image among them; a font,
    // a list, a form and the image's space used; two ranges; five names
    // and the header's list read from operands; and the walk hears the
    // names and the list's keys and names, nine.
    assert_eq!(
        columns(&read[0]),
        [
            "fixture.pdf",
            "1",
            "ok",
            "32",
            "#",
            "19",
            "0,0",
            "4:#",
            "2:#",
            "6:#",
            "9:#",
            "2,2:#",
            "1/2:#",
        ]
    );
    // The form, read with its page's resources, having none of its own.
    let form = columns(&read[1]);
    assert!(
        form[1].starts_with("1 form ") && form[1].ends_with(" 0"),
        "{form:?}"
    );
    assert_eq!(
        form[2..],
        [
            "ok", "10", "#", "4", "0,0", "0:#", "1:#", "2:#", "2:#", "-", "-"
        ]
    );
    let read = lines(DUMP_UNENDED);
    assert_eq!(read.len(), 1, "{read:#?}");
    assert_eq!(
        columns(&read[0])[2..],
        [
            "string",
            "4",
            "#",
            "2",
            "-",
            "-",
            "err:string",
            "1:#",
            "2:#,unread",
            "err:string",
            "0/0:#",
        ]
    );
}
