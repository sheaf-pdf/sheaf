//! A page's blocks (§13): its content classified by who drew what. The
//! running heads, watermarks and backgrounds marked as pagination
//! artifacts, and which tool drew each; sheaf's own blocks, by the marker
//! on their stream or in their property list; the streams that isolate
//! the page's content; and the content's `q`/`Q` balance. A source reads
//! a page's once, when first asked ([`Source::blocks`]), never at open.
//! The content is read by pdf-content-streams' operations, its marked
//! ranges and balance found as they're read ([`MarkedNest`],
//! [`Balance`]); what's classified is sheaf's, and of the ranges it
//! keeps it holds what it reads of them ([`KeptRange`]), as a source
//! keeps a page's blocks past the property lists the family's borrow.
//!
//! [`Source::blocks`]: crate::source::Source::blocks

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::ops::Range;
use std::sync::{Arc, Mutex, PoisonError};

use crate::pdf::content::{self as family, Balance, MarkedEvent, MarkedNest};
use crate::pdf::{Dictionary, Object, ObjectId, StreamExt, Value};

use super::page::{
    Concat, ContentFault, PageContent, ReadObjects, SpanAcross, direct, properties, res_cx,
};
use super::{Operation, ResCx, SpliceError, SyntaxError, Tok, tokens};
use crate::source::Limits;

/// A marked range a page's blocks keep ([`PageBlocks::marked`]), an
/// artifact's or one of sheaf's blocks': what sheaf reads of the range
/// pdf-content-streams found, held by value, so it outlives the lists it
/// was read with.
#[derive(Clone, PartialEq)]
pub struct KeptRange {
    pub tag: Vec<u8>,
    /// Its property list, written in the content or the one the
    /// resources have under the name written there: None for `BMC`, a
    /// name the resources have no list under, or what's neither a
    /// dictionary nor a name.
    pub list: Option<Dictionary>,
    /// Whether its list is named, `/Tag /Name BDC`, so it's the
    /// resources' and not the content's to write again.
    pub named: bool,
    /// From its tag, before `BMC` or `BDC`, through its `EMC`.
    pub span: SpanAcross,
}

impl KeptRange {
    /// `found`, its list copied.
    fn of(found: &family::Marked<'_>) -> KeptRange {
        KeptRange {
            tag: found.tag.clone(),
            list: list(found).cloned(),
            named: matches!(found.props, Some(family::Props::Named(..))),
            span: found.span,
        }
    }
}

impl fmt::Debug for KeptRange {
    /// Without its list's entries, which may be a page's text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeptRange")
            .field("tag", &String::from_utf8_lossy(&self.tag))
            .field("list", &self.list.as_ref().map(Dictionary::len))
            .field("named", &self.named)
            .field("span", &self.span)
            .finish()
    }
}

/// What a pagination artifact is, by its `/Subtype`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PaginationKind {
    Header,
    Footer,
    Watermark,
    Background,
}

impl PaginationKind {
    pub const ALL: [PaginationKind; 4] = [
        PaginationKind::Header,
        PaginationKind::Footer,
        PaginationKind::Watermark,
        PaginationKind::Background,
    ];

    /// Its `/Subtype`, which Acrobat's `/Private` names too.
    pub fn name(self) -> &'static [u8] {
        match self {
            PaginationKind::Header => b"Header",
            PaginationKind::Footer => b"Footer",
            PaginationKind::Watermark => b"Watermark",
            PaginationKind::Background => b"Background",
        }
    }

    pub fn from_name(name: &[u8]) -> Option<PaginationKind> {
        PaginationKind::ALL.into_iter().find(|k| k.name() == name)
    }
}

/// Who drew a pagination artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PagOrigin {
    /// sheaf, as a generator's block: in a `/SHEF_Block` stream, or with
    /// `/SHEF_Gen` in its list.
    Sheaf,
    /// Acrobat: it draws Form XObjects only, each with `/PieceInfo
    /// /ADBE_CompoundType /Private` a pagination kind.
    Acrobat,
    /// Any other: the author's own running heads, as Word, LibreOffice and
    /// InDesign mark them.
    Authored,
}

impl PagOrigin {
    /// Whose a generator's `replaces` takes out on the pages it covers
    /// (§11.4), and RemoveWatermark and RemoveHeaderFooter by default:
    /// every tool's but the author's own, which only `include_authored`
    /// asks for (proposal 31).
    pub const REPLACED: [PagOrigin; 2] = [PagOrigin::Sheaf, PagOrigin::Acrobat];
}

/// A generator's id: random in [1, 2^53), as an id a file will hold is
/// (M10.5 §4), so two files' never meet. sheaf writes it in a block as
/// `(g<hex>)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GenId(pub u64);

/// A layer's id: random in [1, 2^53), as a generator's is. sheaf writes it
/// in a block as `(b<hex>)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u64);

impl GenId {
    pub fn parse(s: &[u8]) -> Option<GenId> {
        hex_id(s, b'g').map(GenId)
    }

    /// One drawn from `rng` that `taken` says no generator has.
    pub(crate) fn draw(rng: &mut fastrand::Rng, taken: impl Fn(GenId) -> bool) -> GenId {
        loop {
            let id = GenId(rng.u64(1..1 << 53));
            if !taken(id) {
                return id;
            }
        }
    }
}

/// As a block names it: `g` and its hex, which [`GenId::parse`] reads.
impl fmt::Display for GenId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "g{:x}", self.0)
    }
}

impl BlockId {
    pub fn parse(s: &[u8]) -> Option<BlockId> {
        hex_id(s, b'b').map(BlockId)
    }

    /// One drawn from `rng` that `taken` says isn't taken.
    pub(crate) fn draw(rng: &mut fastrand::Rng, taken: impl Fn(BlockId) -> bool) -> BlockId {
        loop {
            let id = BlockId(rng.u64(1..1 << 53));
            if !taken(id) {
                return id;
            }
        }
    }
}

/// As a block names it: `b` and its hex, which [`BlockId::parse`] reads.
impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "b{:x}", self.0)
    }
}

/// `prefix` and up to 16 hex digits, the number they write.
fn hex_id(s: &[u8], prefix: u8) -> Option<u64> {
    let digits = s.strip_prefix(&[prefix])?;
    if digits.is_empty() || digits.len() > 16 || !digits.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    u64::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()
}

/// A block of sheaf's (§11.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SheafBlock {
    /// A generator's output on the page.
    Gen(GenId),
    /// An OCR layer.
    Ocr(BlockId),
    /// Placements flattened into the page.
    Flat(BlockId),
}

/// Where a block of sheaf's is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockAt {
    /// A stream of its own, marked `/SHEF_Block`.
    Stream(ObjectId),
    /// A marked range in a stream another tool merged it into.
    Marked(SpanAcross),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SheafBlockRef {
    pub block: SheafBlock,
    pub at: BlockAt,
}

/// A pagination artifact: an `/Artifact` range whose `/Subtype` is a
/// pagination kind, outside any other. Acrobat, Word and sheaf write
/// `/Type /Pagination` with it; without a `/Subtype` there's no kind to
/// remove it by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pagination {
    pub kind: PaginationKind,
    pub origin: PagOrigin,
    /// The generator whose block it is, by its stream's marker or its
    /// list's `/SHEF_Gen`: sheaf's own.
    pub generator: Option<GenId>,
    pub span: SpanAcross,
}

impl Pagination {
    /// What to take out of `content`, its tokens read with `res`, to
    /// remove it: its span, but for a run of `Q`s right after its `BDC`,
    /// which end a `q` from before it and stay. Acrobat opens its first
    /// artifact on a page with the `Q` that ends the `q` it put before the
    /// page's content; kept, it leaves the page as sheaf's own header
    /// does, its `Q` outside its range. The two cuts either side are
    /// spliced together, the marked content one opens closed by the other.
    pub fn cuts(
        &self,
        content: &Concat,
        res: &ResCx,
        lim: &Limits,
    ) -> Result<Vec<SpanAcross>, SyntaxError> {
        let Some(range) = content.offsets(self.span) else {
            return Ok(vec![self.span]);
        };
        let at = |e: SyntaxError| SyntaxError {
            at: e.at + range.start,
            ..e
        };
        let family = lim.content.family();
        let mut ops = tokens(&content.bytes()[range.clone()], res, &family)
            .filter(|t| !matches!(t, Ok((_, Tok::Operand(_)))));
        // Its BMC or BDC, then the run of `Q`s.
        ops.next().transpose().map_err(at)?;
        let mut kept: Option<Range<usize>> = None;
        for token in ops {
            match token.map_err(at)? {
                (r, Tok::Op(b"Q")) => {
                    kept = Some(kept.map_or(r.clone(), |k| k.start..r.end));
                }
                _ => break,
            }
        }
        Ok(match kept {
            None => vec![self.span],
            Some(q) => vec![
                content.span(range.start..range.start + q.start),
                content.span(range.start + q.end..range.end),
            ],
        })
    }

    /// Whether it's of one of `kinds`, drawn by one of `origins`.
    pub fn selected_by(
        &self,
        kinds: &BTreeSet<PaginationKind>,
        origins: &BTreeSet<PagOrigin>,
    ) -> bool {
        kinds.contains(&self.kind) && origins.contains(&self.origin)
    }
}

/// A page's content, classified.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageBlocks {
    /// The artifact ranges, at any depth, and sheaf's marked blocks, in
    /// the order they open; not the structure's ranges, which a tagged
    /// page has one of for every run of text.
    pub marked: Arc<[KeptRange]>,
    pub pagination: Arc<[Pagination]>,
    /// sheaf's blocks: each stream marked `/SHEF_Block`, and each range
    /// whose list has `/SHEF_Gen` or `/SHEF_OcrBlock` in a stream that
    /// isn't.
    pub sheaf: Arc<[SheafBlockRef]>,
    /// The streams marked `/SHEF_Block << /K /Isolate >>`.
    pub isolation: Arc<[ObjectId]>,
    /// Whether a stream of its content is marked as sheaf's
    /// ([`STREAM_KEYS`](crate::private::STREAM_KEYS)): `/SHEF_Block`,
    /// whatever else it says.
    pub sheaf_marked: bool,
    pub balance: Balance,
    /// Whether a Form XObject the page draws outside its pagination
    /// artifacts has one inside: removing the page's leaves it in place.
    /// Forms are read one level deep, each once, within what's left of
    /// the page's [`ContentLimits::decompressed`] after its streams: one
    /// that can't be read draws nothing, as in PDFium, and has none, but
    /// one past the cap, met once the page's share is spent, or drawn with
    /// resources that don't read but for the file's damage, may have one,
    /// and counts as having one.
    ///
    /// [`ContentLimits::decompressed`]: crate::source::ContentLimits
    pub nested_in_forms: bool,
}

/// A page whose content can't be read, or can't be changed as asked: what
/// an op that needs it reports, naming the page.
#[derive(Debug, thiserror::Error)]
pub enum ContentError {
    #[error("page {}'s content can't be read; nothing was changed", .page + 1)]
    Read {
        /// From 0.
        page: u32,
        #[source]
        fault: ContentFault,
    },
    #[error(
        "page {}'s content can't be changed without changing what else it draws; nothing was changed",
        .page + 1
    )]
    Unsafe {
        page: u32,
        #[source]
        error: SpliceError,
    },
}

impl ContentError {
    /// The page it names, from 0.
    pub fn page(&self) -> u32 {
        match *self {
            ContentError::Read { page, .. } | ContentError::Unsafe { page, .. } => page,
        }
    }

    /// The same error naming page `page`: a document's, where a source's
    /// page was named.
    pub fn on_page(mut self, to: u32) -> ContentError {
        match &mut self {
            ContentError::Read { page, .. } | ContentError::Unsafe { page, .. } => *page = to,
        }
        self
    }
}

impl PageBlocks {
    /// The blocks of a page whose `/Contents` names `contents`, drawn with
    /// `resources`, its streams read without keeping them; what's known
    /// of the forms it draws is in `known`.
    pub fn read(
        objects: &impl ReadObjects,
        contents: &[ObjectId],
        resources: Option<&Object>,
        lim: &Limits,
        known: &DrawnForms,
    ) -> Result<PageBlocks, ContentFault> {
        let content = PageContent::read(objects, contents, lim)?;
        PageBlocks::of(objects, &content, resources, lim, known)
    }

    /// The blocks of `content`, drawn with `resources`; what's known of
    /// the forms it draws is in `known`.
    pub fn of(
        objects: &impl ReadObjects,
        content: &PageContent,
        resources: Option<&Object>,
        lim: &Limits,
        known: &DrawnForms,
    ) -> Result<PageBlocks, ContentFault> {
        let concat = content.concat();
        let res = res_cx(objects, resources, lim).map_err(ContentFault::Resources)?;
        let props = properties(objects, resources);
        let family = lim.content.family();
        let mut scan = Scan {
            nest: MarkedNest::new(&props, &family),
            balance: Balance::default(),
            open: None,
            drawn: Vec::new(),
            forms: BTreeSet::new(),
        };
        for op in concat.operations(&res, &family) {
            let op = op.map_err(|e| content.fault(e))?;
            scan.step(concat, &op).map_err(|e| content.fault(e))?;
        }
        let Scan {
            nest,
            balance,
            drawn,
            forms,
            ..
        } = scan;
        let marked = nest.done().map_err(|e| content.fault(e))?;
        let marks: Vec<StreamMark> = content.dicts().iter().map(StreamMark::of).collect();
        let mark_of = |m: &family::Marked| marks.get(m.span.start.stream as usize);
        let pagination = drawn
            .iter()
            .map(|d| {
                let range = &marked[d.index];
                let mark = mark_of(range).unwrap_or(&StreamMark::Plain);
                let generator = match (mark, marked_block(list(range))) {
                    (StreamMark::Block(SheafBlock::Gen(g)), _) => Some(*g),
                    (_, Some(SheafBlock::Gen(g))) => Some(g),
                    _ => None,
                };
                Pagination {
                    kind: d.kind,
                    origin: d.origin(list(range), mark, |form| {
                        xobject(objects, resources, form)
                            .is_some_and(|id| known.acrobat(objects, id))
                    }),
                    generator,
                    span: range.span,
                }
            })
            .collect();
        let ids = content.ids();
        let mut sheaf: Vec<SheafBlockRef> = ids
            .iter()
            .zip(&marks)
            .filter_map(|(&id, mark)| match *mark {
                StreamMark::Block(block) => Some(SheafBlockRef {
                    block,
                    at: BlockAt::Stream(id),
                }),
                _ => None,
            })
            .collect();
        sheaf.extend(marked.iter().filter_map(|m| {
            if matches!(mark_of(m), Some(StreamMark::Block(_))) {
                return None;
            }
            Some(SheafBlockRef {
                block: marked_block(list(m))?,
                at: BlockAt::Marked(m.span),
            })
        }));
        let isolation = ids
            .iter()
            .zip(&marks)
            .filter(|(_, mark)| matches!(mark, StreamMark::Isolate))
            .map(|(&id, _)| id)
            .collect();
        let forms: BTreeSet<ObjectId> = forms
            .iter()
            .filter_map(|form| xobject(objects, resources, form))
            .collect();
        let mut budget = lim
            .content
            .decompressed
            .saturating_sub(concat.bytes().len());
        let nested_in_forms = forms
            .into_iter()
            .any(|form| known.nested(objects, form, resources, lim, &mut budget));
        Ok(PageBlocks {
            marked: (marked.iter())
                .filter(|m| m.tag == b"Artifact" || marked_block(list(m)).is_some())
                .map(KeptRange::of)
                .collect(),
            pagination,
            sheaf: sheaf.into(),
            isolation,
            sheaf_marked: (content.dicts().iter())
                .any(|d| (d.iter()).any(|(k, _)| crate::private::STREAM_KEYS.contains(&k))),
            balance,
            nested_in_forms,
        })
    }

    /// Whether it has a pagination artifact of one of `kinds`.
    pub fn has(&self, kinds: &[PaginationKind]) -> bool {
        self.pagination.iter().any(|p| kinds.contains(&p.kind))
    }

    /// Its pagination artifacts of `kinds` that `origins` drew: what a
    /// filter, a generator's `replaces` and a removal select (§3.4).
    pub fn selected<'a>(
        &'a self,
        kinds: &'a BTreeSet<PaginationKind>,
        origins: &'a BTreeSet<PagOrigin>,
    ) -> impl Iterator<Item = &'a Pagination> + 'a {
        (self.pagination.iter()).filter(|p| p.selected_by(kinds, origins))
    }

    /// The kind of pagination artifact generator `id`'s blocks draw here,
    /// by their property lists: None when none draws one.
    pub fn kind_of(&self, id: GenId) -> Option<PaginationKind> {
        (self.marked.iter())
            .filter(|m| marked_block(m.list.as_ref()) == Some(SheafBlock::Gen(id)))
            .find_map(|m| pagination(&m.tag, m.list.as_ref()))
    }
}

/// What a stream's dictionary marks it as.
enum StreamMark {
    Plain,
    Block(SheafBlock),
    Isolate,
}

impl StreamMark {
    fn of(dict: &Dictionary) -> StreamMark {
        let Some(Value::Dict(block)) = dict.get(b"SHEF_Block").map(Object::value) else {
            return StreamMark::Plain;
        };
        let string = |key: &[u8]| block.get(key)?.as_bytes();
        let mark = match block.get(b"K").and_then(Object::as_name) {
            Some(b"Gen") => string(b"G")
                .and_then(GenId::parse)
                .map(|g| StreamMark::Block(SheafBlock::Gen(g))),
            Some(b"Ocr") => string(b"B")
                .and_then(BlockId::parse)
                .map(|b| StreamMark::Block(SheafBlock::Ocr(b))),
            Some(b"Flat") => string(b"B")
                .and_then(BlockId::parse)
                .map(|b| StreamMark::Block(SheafBlock::Flat(b))),
            Some(b"Isolate") => Some(StreamMark::Isolate),
            _ => None,
        };
        mark.unwrap_or(StreamMark::Plain)
    }
}

/// What sheaf marked a `/Contents` stream as, by its dictionary alone
/// (§11.4): a generator's block, with the digests of its layout (`/L`) and
/// of what it was drawn from (`/R`), each None when it isn't 16 bytes, and
/// the page's Bates number (`/N`, decimal digits), when it has one; a
/// layer's block, of recognized text or of annotations a flatten drew, by
/// its layer's id (`/B`); or a stream that isolates the page's own
/// content. What a source reads of a page's streams without decompressing
/// them ([`Source::markers`]).
///
/// [`Source::markers`]: crate::source::Source::markers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Marker {
    Gen {
        id: GenId,
        layout: Option<u128>,
        recipe: Option<u128>,
        number: Option<u64>,
    },
    Ocr(BlockId),
    Flat(BlockId),
    Isolate,
}

impl Marker {
    /// What `dict`, a stream's dictionary, marks it as, if anything.
    pub fn of(dict: &Dictionary) -> Option<Marker> {
        match StreamMark::of(dict) {
            StreamMark::Block(SheafBlock::Gen(id)) => {
                let block = dict.get(b"SHEF_Block").and_then(Object::as_dict);
                let string = |key: &[u8]| block?.get(key)?.as_bytes();
                let digest = |key: &[u8]| Some(u128::from_be_bytes(string(key)?.try_into().ok()?));
                let number = string(b"N")
                    .filter(|n| (1..=20).contains(&n.len()) && n.iter().all(u8::is_ascii_digit))
                    .and_then(|n| std::str::from_utf8(n).ok()?.parse().ok());
                Some(Marker::Gen {
                    id,
                    layout: digest(b"L"),
                    recipe: digest(b"R"),
                    number,
                })
            }
            StreamMark::Block(SheafBlock::Ocr(id)) => Some(Marker::Ocr(id)),
            StreamMark::Block(SheafBlock::Flat(id)) => Some(Marker::Flat(id)),
            StreamMark::Isolate => Some(Marker::Isolate),
            StreamMark::Plain => None,
        }
    }
}

/// What a stream that isolates content holds, decoded: its one operator
/// (`q` or `Q`) and how many of it. None for anything else, as a stream
/// marked [`Marker::Isolate`] that another tool changed may hold, which is
/// the page's own content.
pub fn isolates(stream: &[u8]) -> Option<(u8, u32)> {
    let space = |b: &u8| matches!(b, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ');
    let mut ops = stream.split(space).filter(|t| !t.is_empty());
    let op = match ops.next()? {
        b"q" => b'q',
        b"Q" => b'Q',
        _ => return None,
    };
    let mut n = 1u32;
    for t in ops {
        if t != [op] {
            return None;
        }
        n = n.checked_add(1)?;
    }
    Some((op, n))
}

/// A marked range's property list, wherever it's written, as
/// pdf-content-streams found it.
fn list<'m>(m: &'m family::Marked<'_>) -> Option<&'m Dictionary> {
    m.props.as_ref().and_then(family::Props::dict)
}

/// The block of sheaf's a range's list marks: `/SHEF_Gen` on a
/// generator's, `/SHEF_OcrBlock` on an OCR layer's.
fn marked_block(list: Option<&Dictionary>) -> Option<SheafBlock> {
    let list = list?;
    let string = |key: &[u8]| list.get(key)?.as_bytes();
    string(b"SHEF_Gen")
        .and_then(GenId::parse)
        .map(SheafBlock::Gen)
        .or_else(|| {
            string(b"SHEF_OcrBlock")
                .and_then(BlockId::parse)
                .map(SheafBlock::Ocr)
        })
}

/// The kind of pagination artifact a range tagged `tag`, with `list`, is,
/// if it's one.
fn pagination(tag: &[u8], list: Option<&Dictionary>) -> Option<PaginationKind> {
    if tag != b"Artifact" {
        return None;
    }
    PaginationKind::from_name(list?.get(b"Subtype")?.as_name()?)
}

/// A pagination artifact being read, outside any other: what it draws.
struct Drawn {
    /// Where it is in the marked ranges.
    index: usize,
    kind: PaginationKind,
    /// Whether it paints anything but XObjects.
    paints: bool,
    /// The XObjects it draws.
    forms: BTreeSet<Vec<u8>>,
}

impl Drawn {
    /// Who drew it, its range's list `list`, in a stream marked `mark`;
    /// `acrobat` says whether a form it draws is Acrobat's (§13's
    /// classification).
    fn origin(
        &self,
        list: Option<&Dictionary>,
        mark: &StreamMark,
        acrobat: impl Fn(&[u8]) -> bool,
    ) -> PagOrigin {
        let has = |key: &[u8]| list.is_some_and(|l| l.has(key));
        match mark {
            StreamMark::Block(_) => PagOrigin::Sheaf,
            _ if has(b"SHEF_Gen") => PagOrigin::Sheaf,
            _ if !self.paints
                && !self.forms.is_empty()
                && self.forms.iter().all(|f| acrobat(f)) =>
            {
                PagOrigin::Acrobat
            }
            _ => PagOrigin::Authored,
        }
    }
}

/// A page's content as it's stepped through, an operation at a time.
struct Scan<'p> {
    nest: MarkedNest<'p>,
    balance: Balance,
    open: Option<Drawn>,
    drawn: Vec<Drawn>,
    /// The XObjects drawn outside pagination artifacts.
    forms: BTreeSet<Vec<u8>>,
}

impl Scan<'_> {
    /// The page after `op`, its next operation, read from `concat`: an
    /// inline image's is named `BI`.
    fn step(&mut self, concat: &Concat, op: &Operation) -> Result<(), SyntaxError> {
        match self.nest.step(concat, op)? {
            Some(MarkedEvent::Opened(index)) if self.open.is_none() => {
                let range = self.nest.get(index);
                if let Some(kind) = range.and_then(|m| pagination(&m.tag, list(m))) {
                    self.open = Some(Drawn {
                        index,
                        kind,
                        paints: false,
                        forms: BTreeSet::new(),
                    });
                }
            }
            Some(MarkedEvent::Ended(index)) => {
                if let Some(drawn) = self.open.take_if(|d| d.index == index) {
                    self.drawn.push(drawn);
                }
            }
            _ => {}
        }
        self.balance.take(op);
        let name = op.name();
        let form = || op.arg(0).and_then(|o| o.name()).map(|f| f.into_owned());
        let Some(drawn) = &mut self.open else {
            if name == b"Do"
                && let Some(form) = form()
            {
                self.forms.insert(form);
            }
            return Ok(());
        };
        match name {
            b"Do" => drawn.forms.extend(form()),
            name => drawn.paints |= paints(name),
        }
        Ok(())
    }
}

/// Whether `op` paints: a path, text, a shading or an inline image (`BI`).
fn paints(op: &[u8]) -> bool {
    matches!(
        op,
        b"S" | b"s"
            | b"f"
            | b"F"
            | b"f*"
            | b"B"
            | b"B*"
            | b"b"
            | b"b*"
            | b"Tj"
            | b"TJ"
            | b"'"
            | b"\""
            | b"sh"
            | b"BI"
    )
}

/// XObject `name` of `resources`.
fn xobject(
    objects: &impl ReadObjects,
    resources: Option<&Object>,
    name: &[u8],
) -> Option<ObjectId> {
    let resources = direct(objects, resources?)?.as_dict()?;
    let xobjects = direct(objects, resources.get(b"XObject")?)?;
    xobjects.as_dict()?.get(name)?.as_reference()
}

/// What's known of the XObjects pages draw, by object, so each is read
/// once however many names and pages draw it: whether it's a form Acrobat
/// drew a pagination artifact in, and whether one is inside it. A source
/// keeps one for its pages. Whether a form that draws with its page's
/// resources, having none of its own, has one depends on the page, so
/// it's read for each page that draws it.
#[derive(Debug, Default)]
pub struct DrawnForms {
    acrobat: Mutex<HashMap<ObjectId, bool>>,
    nested: Mutex<HashMap<ObjectId, bool>>,
}

impl DrawnForms {
    /// Whether XObject `id` is a form Acrobat drew a pagination artifact
    /// in: its `/PieceInfo /ADBE_CompoundType /Private` names a pagination
    /// kind.
    fn acrobat(&self, objects: &impl ReadObjects, id: ObjectId) -> bool {
        if let Some(&known) = lock(&self.acrobat).get(&id) {
            return known;
        }
        let kind = objects.read(id, |form| {
            let dict = &form.as_stream()?.dict;
            let get = |d: &Dictionary, key: &[u8]| direct(objects, d.get(key)?).cloned();
            let piece = get(dict, b"PieceInfo")?;
            let compound = get(piece.as_dict()?, b"ADBE_CompoundType")?;
            let private = get(compound.as_dict()?, b"Private")?;
            PaginationKind::from_name(private.as_name()?)
        });
        let acrobat = matches!(kind, Ok(Some(_)));
        lock(&self.acrobat).insert(id, acrobat);
        acrobat
    }

    /// Whether XObject `id` is a form with a pagination artifact in it,
    /// drawn on a page with `resources`. Reading it takes its size from
    /// `budget`, what's left of the page's; with none left, it counts as
    /// having one, as it does where the resources it's read with don't
    /// read but for the file's damage ([`res_cx`]), which isn't kept: a
    /// read of them again may.
    fn nested(
        &self,
        objects: &impl ReadObjects,
        id: ObjectId,
        resources: Option<&Object>,
        lim: &Limits,
        budget: &mut usize,
    ) -> bool {
        if let Some(&known) = lock(&self.nested).get(&id) {
            return known;
        }
        if *budget == 0 {
            return true;
        }
        let read = objects.read(id, |form| {
            let form = form.as_stream()?;
            if form.dict.get(b"Subtype").and_then(Object::as_name)? != b"Form" {
                return None;
            }
            let own = form.dict.get(b"Resources").cloned();
            Some(
                match form.decompressed_content_with_limit(lim.content.decompressed) {
                    Ok(data) => Ok((data, own)),
                    Err(crate::pdf::Error::Limit(crate::pdf::Limit::Decompressed)) => Err(true),
                    Err(_) => Err(false),
                },
            )
        });
        let (data, own) = match read {
            Ok(Some(Ok(form))) => form,
            // Past the cap, it may have one; not a form, or unreadable,
            // it draws nothing.
            Ok(Some(Err(too_large))) => return self.knows(id, too_large),
            Ok(None) | Err(_) => return self.knows(id, false),
        };
        *budget = budget.saturating_sub(data.len());
        // A form without resources of its own draws with its page's.
        let resources = own.as_ref().or(resources);
        let content = Concat::new(&[&data]);
        // Resources that don't read, but for the file's damage, may hide
        // one.
        let Ok(res) = res_cx(objects, resources, lim) else {
            return true;
        };
        let props = properties(objects, resources);
        let nested = family::marked(&content, &props, &res, &lim.content.family())
            .is_ok_and(|found| found.iter().any(|m| pagination(&m.tag, list(m)).is_some()));
        if own.is_some() {
            self.knows(id, nested);
        }
        nested
    }

    /// `nested` for form `id`, kept.
    fn knows(&self, id: ObjectId, nested: bool) -> bool {
        lock(&self.nested).insert(id, nested);
        nested
    }

    /// Whether form `id` is known to have a pagination artifact in it, or
    /// not.
    #[cfg(test)]
    pub(crate) fn nested_known(&self, id: ObjectId) -> Option<bool> {
        lock(&self.nested).get(&id).copied()
    }
}

fn lock<T>(lock: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}
