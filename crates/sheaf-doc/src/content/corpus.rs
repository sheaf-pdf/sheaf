//! The content toolkit over Greg's corpus of real forms
//! ([`forms::corpus`](crate::forms::corpus)): every page's content, and
//! every form XObject it draws, reads, lexes into tokens that tile it and
//! splices back to each stream's own bytes, read apart; its marked content
//! balances; its `q`/`Q` balance is what `Content::decode` counts;
//! its words decode through its fonts; its pagination artifacts are
//! classified as the tools that made the files drew them; each of them
//! splices out, as a strip cuts it, or is refused, Acrobat's never; and
//! each list of theirs written in the content can be written again without
//! a key, as Clean Copy writes sheaf's without what marks it.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::ControlFlow;
use std::path::Path;
use std::time::Instant;

use crate::pdf::{Object, ObjectId, StreamExt};

use super::interp::{Kind, Opts, interpret};
use super::tests::blank;
use super::*;
use crate::forms::corpus::{Corpus, Entry, each_entry};
use crate::source::Limits;
use crate::store::{BlobKind, Store};

const LIM: &Limits = &Limits::DEFAULT;

/// What one file's content came to.
#[derive(Default)]
struct Outcome {
    pages: usize,
    forms: usize,
    bytes: usize,
    /// Content that can't be read or lexed, or whose tokens don't tile
    /// it, with its page: each a line.
    unread: Vec<String>,
    /// Pages whose marked content doesn't balance.
    unbalanced: Vec<String>,
    /// Streams the concatenation, or a splice of nothing, changed from
    /// their own bytes.
    changed: Vec<String>,
    /// Pages `Content::decode` counts `q` and `Q` on (no inline images, and decoded),
    /// and those whose count differs.
    counted: usize,
    miscounted: Vec<String>,
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

/// Where `s`'s tokens, lexed with `res`, don't tile it: an error, a
/// token out of order, or something between or after them that isn't
/// white-space or a comment.
fn untiled(s: &[u8], res: &ResCx) -> Option<String> {
    let mut at = 0;
    for token in lex(s, res, LIM) {
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

/// `Content::decode`'s count of `bytes`' `q`s and `Q`s, the reference.
fn decoded_balance(bytes: &[u8]) -> Option<Balance> {
    let content = crate::pdf::content::Content::decode(bytes).ok()?;
    let mut balance = Balance::default();
    for op in &content.operations {
        balance.take(op.operator.as_bytes());
    }
    Some(balance)
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
    // The splice reads with pdf-content-streams' limits.
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
        let res = ResCx::read(src.pdf(), resources);
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
        match splice(concat, res.family(), cs, &[]) {
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
        let lexed: Vec<_> = lex(concat.bytes(), &res, LIM).map(Result::unwrap).collect();
        let has_image = lexed.iter().any(|(_, t)| matches!(t, Tok::InlineImage(_)));
        let ours = balance(concat, &res, LIM).unwrap();
        if !has_image && let Some(reference) = decoded_balance(concat.bytes()) {
            out.counted += 1;
            if ours != reference {
                out.miscounted
                    .push(format!("{at}: {ours:?}, decoded {reference:?}"));
            }
        }
        out.words += text_bag(src.pdf(), concat, resources, LIM, &fonts)
            .unwrap()
            .len();
        // The forms it draws, each once a file.
        let names = names_used(concat, &res, LIM, &[]).unwrap();
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
            let res = ResCx::read(src.pdf(), own.or(resources));
            if let Some(wrong) = untiled(&data, &res) {
                out.unread.push(format!("{at}: tokens don't tile {wrong}"));
                continue;
            }
            let one = Concat::new(&[&data]);
            match splice(&one, res.family(), cs, &[]) {
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
                    let made = splice(concat, res.family(), cs, &cuts).is_ok();
                    *out.splices.entry((p.origin, made)).or_default() += 1;
                    if p.origin == PagOrigin::Acrobat && !made {
                        out.acrobat_refused.push(format!("{at}: {:?}", p.kind));
                    }
                    let range = blocks.marked.iter().find(|m| m.span == p.span);
                    let inline = range.is_some_and(|m| matches!(m.props, Some(Props::Inline(_))));
                    let made = match without_keys(concat, res.family(), cs, p.span, &[b"Type"]) {
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
                    if let Err(e) = splice(concat, res.family(), cs, &cuts) {
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
/// draws: read, lexed, spliced with nothing taken out, counted,
/// classified, and spliced without each pagination artifact.
#[test]
fn corpus_content_lexes_splices_and_classifies() {
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
    let (changed, miscounted) = (all(|o| &o.changed), all(|o| &o.miscounted));
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
         {} unread, {} with marked content unbalanced, {} decoded counted, {} nested in forms",
        sum(|o| o.pages),
        sum(|o| o.bytes) >> 20,
        sum(|o| o.forms),
        entries.len(),
        unread.len(),
        unbalanced.len(),
        sum(|o| o.counted),
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
    assert!(miscounted.is_empty(), "{miscounted:#?}");
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
pub(super) fn dump_picks(corpus: &Corpus) -> BTreeMap<String, &'static str> {
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
    }
}

/// The dump's number for what was read of a font's program.
fn program_number(program: FontProgram) -> u64 {
    match program {
        FontProgram::None => 0,
        FontProgram::Read => 1,
        FontProgram::Malformed => 2,
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
        let read = interpret(
            src.pdf(),
            concat,
            resources,
            LIM,
            &fonts,
            Opts::default(),
            |event| {
                let Kind::Text(show) = event.kind else {
                    return ControlFlow::Continue(());
                };
                records.begin("show");
                let id = show.font.and_then(Foreign::id);
                records.maybe("font", id.map(|id| u64::from(id.number)));
                records.maybe("generation", id.map(|id| u64::from(id.generation)));
                records.maybe("kind", show.font.map(|f| kind_number(f.kind())));
                records.maybe("program", show.font.map(|f| program_number(f.program())));
                records.number("has_font", u64::from(show.font.is_some()));
                records.approx(show.font.map(Foreign::approx).unwrap_or_default());
                records.number(
                    "vertical",
                    u64::from(show.font.is_some_and(Foreign::is_vertical)),
                );
                records.number("stock", u64::from(show.stock));
                records.bytes("string", Some(show.string));
                records.end();
                for g in show.glyphs() {
                    glyphs += 1;
                    records.begin("glyph");
                    records.number("code", u64::from(g.code));
                    records.maybe("cid", g.cid.map(u64::from));
                    let [a, b, c, d] = g.quad.0;
                    records.bits("quad", &[a.x, a.y, b.x, b.y, c.x, c.y, d.x, d.y]);
                    records.bits("origin", &[g.origin.x, g.origin.y]);
                    records.bits("advance", &[g.advance]);
                    records.approx(g.approx);
                    records.number("invisible", u64::from(g.invisible));
                    records.bytes("text", g.text().as_deref().map(str::as_bytes));
                    records.end();
                }
                ControlFlow::Continue(())
            },
        );
        if let Err(e) = read {
            why.push(format!("interpreting: {e}"));
        }
        let shown = records.finish(&format!("{at}: shown"));

        // Its text bag, and those of the forms its content draws, each
        // judged on its own, as the removal check reads them.
        let mut records = Records::new(full);
        match text_bag(src.pdf(), concat, resources, LIM, &for_text) {
            Ok(bag) => bag_records(&mut records, &bag),
            Err(e) => why.push(format!("text bag: {e}")),
        }
        let res = ResCx::read(src.pdf(), resources);
        let names = names_used(concat, &res, LIM, &[]);
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
                    text_bag(src.pdf(), &Concat::new(&[&data]), own, LIM, &for_text)
                        .map_err(|e| e.to_string())
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
    let mut picks = dump_picks(&corpus);
    let mut sets: BTreeMap<&str, usize> = BTreeMap::new();
    for set in picks.values() {
        *sets.entry(set).or_default() += 1;
    }
    eprintln!("{} files: {sets:?}", picks.len());
    if let Some(only) = &only {
        picks.retain(|path, _| path == only);
        assert_eq!(picks.len(), 1, "{DUMP_ONLY} names no file the dump reads");
    }
    let entries: Vec<Entry> = (picks.keys())
        .map(|path| Entry {
            path: path.clone(),
            ..Entry::default()
        })
        .collect();
    let started = Instant::now();
    let full = only.is_some();
    let read = each_entry(&entries, |entry| {
        let read = || dump_file(&corpus, &entry.path, full);
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(read)).unwrap_or_else(|panic| {
            let said = (panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .or_else(|| panic.downcast_ref::<String>().cloned());
            let said = said.unwrap_or_else(|| "a panic".into());
            let said = said.replace(['\n', '\t'], " ");
            vec![format!("{}\t-\t0\t-\t-\tpanicked: {said}", entry.path)]
        })
    });
    let lines: Vec<String> = read.into_iter().flatten().collect();
    let glyphs: usize = (lines.iter())
        .filter_map(|line| line.split('\t').nth(2)?.parse::<usize>().ok())
        .sum();
    let not_ok = lines.iter().filter(|line| !line.ends_with("\tok")).count();
    eprintln!(
        "{} lines, {glyphs} glyphs, {not_ok} not ok, in {:.0} s",
        lines.len(),
        started.elapsed().as_secs_f64()
    );
    if full {
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
