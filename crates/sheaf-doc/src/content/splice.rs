//! Splicing: a page's streams with ranges taken out and every other byte
//! as it was, never decoded into operations and written again.
//!
//! A range is taken out only when that can't change what's drawn after
//! it (§13), lexically:
//! - it sets no graphics state outside a `q`…`Q` of its own: no `cm`,
//!   color, line style, `gs`, clip or text state (`Tc Tw Tz TL Tf Tr Ts
//!   "`), which would last past it;
//! - outside a text object, its `q`s and `Q`s and its `BT`s and `ET`s
//!   balance on their own;
//! - inside one, it stays inside, its `q`s and `Q`s balance, and it moves
//!   no line (`Td TD Tm T* ' "`), which `q` doesn't save. Text it shows
//!   moved the text on, so text shown after it, before a line is moved or
//!   the object ends, would move back: that refuses it too;
//! - it starts and ends outside a path being made, which what's after it
//!   would paint;
//! - marked content draws nothing, but for optional content (`/OC`),
//!   which shows or hides what it marks: so marked content a range opens,
//!   none of it optional, may be closed by a later one, as long as no
//!   marked content is kept between them, and a range closes only what
//!   it or an earlier one opened. The start and the end of an artifact may
//!   go while what's between stays.
//!
//! Otherwise the splice is refused ([`SpliceError::Unsafe`]), as it is
//! when a range starts or ends inside a token or a comment, or between an
//! operator and its operands, which the next operator would take. Removing
//! a running head drawn in the middle of a text object can't shift the
//! body text.
//!
//! A marked range kept may have its property list written again without
//! some of its keys ([`without_keys`], how a filter takes sheaf's private
//! data out of the lists of its blocks another tool merged into a stream):
//! the list's other bytes, and every other byte of the content, as they
//! were. Only a list written in the content, in one stream, is, and only
//! when what's left of it reads as the list without those keys; otherwise
//! it's refused ([`SpliceError::List`]).

use std::borrow::Cow;
use std::ops::Range;

use super::lex::{self, LexError, ResCx, Tok};
use super::page::{Concat, SpanAcross};
use crate::source::Limits;

/// Why a splice was refused.
#[derive(Debug, thiserror::Error)]
pub enum SpliceError {
    #[error(transparent)]
    Lex(#[from] LexError),
    #[error("{0} is outside the content")]
    OutOfRange(SpanAcross),
    #[error("{0} overlaps another range taken out")]
    Overlap(SpanAcross),
    #[error("taking out {0} would change what's drawn after it")]
    Unsafe(SpanAcross),
    #[error("the property list of the range at {0} can't be written again without the keys asked")]
    List(SpanAcross),
}

/// `content`'s streams with `drop` taken out, lexed with `res`: a stream
/// nothing is taken from is borrowed as it is. Where what's kept either
/// side of a range taken out would join two tokens, a space keeps them
/// apart.
pub fn splice<'a>(
    content: &'a Concat,
    res: &ResCx,
    lim: &Limits,
    drop: &[SpanAcross],
) -> Result<Vec<Cow<'a, [u8]>>, SpliceError> {
    let cuts = checked(content, res, lim, drop)?;
    Ok(written(content, &cuts, &[]))
}

/// What [`splice_each`] takes out of a page's content.
pub struct Spliced {
    /// Whether each group was taken out.
    pub taken: Vec<bool>,
    /// The cuts of those taken, in order.
    cuts: Vec<Cut>,
}

impl Spliced {
    /// Whether where `span` starts in `content` is taken out.
    pub fn takes(&self, content: &Concat, span: SpanAcross) -> bool {
        let at = content.offset(span.start);
        (self.cuts.iter()).any(|c| at.is_some_and(|at| c.range.contains(&at)))
    }

    /// `content`'s streams with what's taken out taken out, and `rewrites`
    /// made in what's kept; one in what's taken out goes with it. None when
    /// no stream changes.
    pub fn streams<'a>(
        &self,
        content: &'a Concat,
        rewrites: &[Rewrite],
    ) -> Option<Vec<Cow<'a, [u8]>>> {
        let rewrites: Vec<&Rewrite> = (rewrites.iter())
            .filter(|r| !(self.cuts.iter()).any(|c| overlap(&c.range, &r.range)))
            .collect();
        (!self.cuts.is_empty() || !rewrites.is_empty())
            .then(|| written(content, &self.cuts, &rewrites))
    }
}

/// `content`'s streams with as many of `groups` taken out as can be, lexed
/// with `res`: each group the cuts that take one range out (as
/// [`Pagination::cuts`] gives them), all of them when [`splice`] takes them
/// together, else those it takes alone, together. What a filter and a
/// generator's `replaces` take out (§11.4), whose ranges a splice refuses
/// stay as they are.
///
/// [`Pagination::cuts`]: super::Pagination::cuts
pub fn splice_each(
    content: &Concat,
    res: &ResCx,
    lim: &Limits,
    groups: &[Vec<SpanAcross>],
) -> Spliced {
    let none = |taken| Spliced {
        taken,
        cuts: Vec::new(),
    };
    if groups.is_empty() {
        return none(Vec::new());
    }
    if let Ok(cuts) = checked(content, res, lim, &groups.concat()) {
        return Spliced {
            taken: vec![true; groups.len()],
            cuts,
        };
    }
    let taken: Vec<bool> = (groups.iter())
        .map(|cuts| checked(content, res, lim, cuts).is_ok())
        .collect();
    let cuts: Vec<SpanAcross> = (groups.iter().zip(&taken))
        .filter(|(_, taken)| **taken)
        .flat_map(|(cuts, _)| cuts.iter().copied())
        .collect();
    if cuts.is_empty() {
        return none(taken);
    }
    match checked(content, res, lim, &cuts) {
        Ok(cuts) => Spliced { taken, cuts },
        Err(_) => none(vec![false; groups.len()]),
    }
}

/// A marked range's property list written again ([`without_keys`]):
/// where it is in the joined bytes, and what it's now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    range: Range<usize>,
    with: Vec<u8>,
}

/// The property list of the `BDC` that opens `span` in `content`, lexed
/// with `res`, without the entries `keys` names, every other byte of it as
/// written: None when it has none of them. Refused when the range doesn't
/// open with a `BDC` whose list is written in the content, in one stream,
/// or what's left of the list doesn't read as it without them.
pub fn without_keys(
    content: &Concat,
    res: &ResCx,
    lim: &Limits,
    span: SpanAcross,
    keys: &[&[u8]],
) -> Result<Option<Rewrite>, SpliceError> {
    let range = content.offsets(span).ok_or(SpliceError::OutOfRange(span))?;
    let refused = || SpliceError::List(span);
    let at = |e: LexError| LexError {
        at: e.at + range.start,
        ..e
    };
    let s = content.bytes();
    // Its operands, the list last, to its `BDC`.
    let mut list = None;
    let mut tokens = lex::lex(&s[range.clone()], res, lim);
    loop {
        match tokens.next().transpose().map_err(at)? {
            Some((r, Tok::Operand(_))) => list = Some(r),
            Some((_, Tok::Op(b"BDC"))) => break,
            _ => return Err(refused()),
        }
    }
    let list = list.map(|r| range.start + r.start..range.start + r.end);
    let list = list
        .filter(|r| s[r.clone()].starts_with(b"<<"))
        .ok_or_else(refused)?;
    let at = content.span(list.clone());
    if at.start.stream != at.end.stream {
        return Err(refused());
    }
    let with =
        lex::without_entries(&s[list.clone()], keys, lim.content.depth).ok_or_else(refused)?;
    Ok((with != s[list.clone()]).then_some(Rewrite { range: list, with }))
}

/// A range to take out, in the joined bytes.
struct Cut {
    range: Range<usize>,
    span: SpanAcross,
}

/// The cuts that take `drop` out of `content`, lexed with `res`, in order,
/// once they're found safe to take out together.
fn checked(
    content: &Concat,
    res: &ResCx,
    lim: &Limits,
    drop: &[SpanAcross],
) -> Result<Vec<Cut>, SpliceError> {
    let mut cuts = drop
        .iter()
        .map(|&span| {
            let range = content.offsets(span).ok_or(SpliceError::OutOfRange(span))?;
            Ok(Cut { range, span })
        })
        .collect::<Result<Vec<_>, SpliceError>>()?;
    cuts.retain(|c| !c.range.is_empty());
    cuts.sort_by_key(|c| c.range.start);
    if let Some(pair) = cuts
        .windows(2)
        .find(|pair| pair[1].range.start < pair[0].range.end)
    {
        return Err(SpliceError::Overlap(pair[1].span));
    }
    check(content, res, lim, &cuts)?;
    Ok(cuts)
}

fn overlap(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

/// A running count of opens and closes: where it is, and the lowest it's
/// been.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Count {
    net: i32,
    min: i32,
}

impl Count {
    fn add(&mut self, by: i32) {
        self.net = self.net.saturating_add(by);
        self.min = self.min.min(self.net);
    }

    fn balanced(self) -> bool {
        self == Count::default()
    }
}

/// What a range taken out holds.
#[derive(Default)]
struct Tally {
    /// Whether it starts inside a text object.
    in_text: bool,
    q: Count,
    text: Count,
    /// Whether it begins or ends a text object.
    text_objects: bool,
    marked: Count,
    /// Whether it opens optional content.
    optional: bool,
    /// Whether it sets graphics state outside a `q`…`Q` of its own.
    sets_state: bool,
    /// Whether it moves a line.
    moves: bool,
    /// Whether it shows text.
    shows: bool,
}

impl Tally {
    /// `op`, and `tag` when it opens marked content.
    fn take(&mut self, op: &[u8], tag: Option<&[u8]>) {
        self.sets_state |= self.q.net == 0 && sets_state(op);
        self.optional |= tag.and_then(lex::name).as_deref() == Some(b"OC");
        match op {
            b"q" => self.q.add(1),
            b"Q" => self.q.add(-1),
            b"BT" => {
                self.text.add(1);
                self.text_objects = true;
            }
            b"ET" => {
                self.text.add(-1);
                self.text_objects = true;
            }
            b"BMC" | b"BDC" => self.marked.add(1),
            b"EMC" => self.marked.add(-1),
            _ => {}
        }
        self.moves |= moves_text(op);
        self.shows |= matches!(op, b"Tj" | b"TJ" | b"'" | b"\"");
    }

    /// Whether taking it out leaves what's drawn after it as it was, but
    /// for text shown after it in its text object, and the marked content
    /// it opens or closes.
    fn safe(&self) -> bool {
        // Optional content left open would show or hide what's kept.
        let hides = self.optional && self.marked.net > 0;
        let kept = self.q.balanced() && !self.sets_state && !hides;
        if self.in_text {
            kept && !self.text_objects && !self.moves
        } else {
            kept && self.text.balanced()
        }
    }
}

/// Whether `op` sets graphics state, which lasts until a `Q` restores it:
/// the CTM, a color or color space, line style, rendering intent,
/// flatness, a `gs`, a clip, or text state.
fn sets_state(op: &[u8]) -> bool {
    matches!(
        op,
        b"cm"
            | b"CS"
            | b"cs"
            | b"SC"
            | b"SCN"
            | b"sc"
            | b"scn"
            | b"G"
            | b"g"
            | b"RG"
            | b"rg"
            | b"K"
            | b"k"
            | b"w"
            | b"J"
            | b"j"
            | b"M"
            | b"d"
            | b"ri"
            | b"i"
            | b"gs"
            | b"W"
            | b"W*"
            | b"Tc"
            | b"Tw"
            | b"Tz"
            | b"TL"
            | b"Tf"
            | b"Tr"
            | b"Ts"
            | b"\""
    )
}

/// Whether `op` moves to a line: sets the text and line matrices, which
/// a text object starts afresh and `q` doesn't save.
fn moves_text(op: &[u8]) -> bool {
    matches!(op, b"Td" | b"TD" | b"Tm" | b"T*" | b"'" | b"\"")
}

/// Whether `op` starts or adds to a path, which the next painting
/// operator paints; and whether it ends one.
fn makes_path(op: &[u8]) -> bool {
    matches!(op, b"m" | b"l" | b"c" | b"v" | b"y" | b"h" | b"re")
}

fn ends_path(op: &[u8]) -> bool {
    matches!(
        op,
        b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"n"
    )
}

/// Whether `cuts` may be taken out of `content`: each starts and ends
/// between steps, outside a comment and a path, and is safe (the module's
/// rules).
fn check(content: &Concat, res: &ResCx, lim: &Limits, cuts: &[Cut]) -> Result<(), SpliceError> {
    let s = content.bytes();
    let mut in_text = false;
    let mut in_path = false;
    // The cut being lexed, and what it holds.
    let mut open: Option<(usize, Tally)> = None;
    // The next cut to start.
    let mut next = 0;
    // A cut that showed text in a text object that hasn't moved a line
    // since.
    let mut shown: Option<SpanAcross> = None;
    // Marked content cuts opened that no cut has closed yet, and the first
    // cut that left some open.
    let mut pending = 0;
    let mut opener = 0;
    // Where the last token ended, and whether it was an operand: a step
    // its operator hasn't ended; and the last two operands, where a tag
    // is.
    let mut after = 0;
    let mut operand = false;
    let mut operands: [Option<&[u8]>; 2] = [None, None];
    let unsafe_cut = |i: usize| SpliceError::Unsafe(cuts[i].span);
    let mut tokens = content.lex(res, lim);
    loop {
        let token = tokens.next().transpose()?;
        let start = token.as_ref().map_or(s.len(), |(r, _)| r.start);
        // The cuts that end or start before this token, in the white-space
        // and comments since the last.
        let between = |at: usize| !in_comment(&s[after..at]);
        if let Some((i, tally)) = open.take_if(|(i, _)| cuts[*i].range.end <= start) {
            let closes_kept = pending + tally.marked.min < 0;
            if operand || in_path || closes_kept || !between(cuts[i].range.end) || !tally.safe() {
                return Err(unsafe_cut(i));
            }
            if pending == 0 && tally.marked.net > 0 {
                opener = i;
            }
            pending += tally.marked.net;
            if tally.in_text && tally.shows {
                shown = Some(cuts[i].span);
            }
        }
        while open.is_none() && next < cuts.len() && cuts[next].range.start <= start {
            let cut = &cuts[next];
            if operand || in_path || !between(cut.range.start) {
                return Err(unsafe_cut(next));
            }
            if cut.range.end <= start && !between(cut.range.end) {
                return Err(unsafe_cut(next));
            }
            if cut.range.end > start {
                open = Some((
                    next,
                    Tally {
                        in_text,
                        ..Tally::default()
                    },
                ));
            }
            next += 1;
        }
        let Some((r, tok)) = token else {
            return match pending {
                0 => Ok(()),
                _ => Err(unsafe_cut(opener)),
            };
        };
        // A token across either end of a cut.
        if let Some((i, _)) = open
            && r.end > cuts[i].range.end
        {
            return Err(unsafe_cut(i));
        }
        if next < cuts.len() && cuts[next].range.start < r.end {
            return Err(unsafe_cut(next));
        }
        after = r.end;
        operand = matches!(tok, Tok::Operand(_));
        let op = match tok {
            Tok::Op(op) => op,
            Tok::Operand(o) => {
                operands = [operands[1], Some(o)];
                continue;
            }
            Tok::InlineImage(_) => {
                operands = [None, None];
                continue;
            }
        };
        let tag = match op {
            b"BMC" => operands[1],
            b"BDC" => operands[0],
            _ => None,
        };
        operands = [None, None];
        match &mut open {
            Some((_, tally)) => tally.take(op, tag),
            None => {
                if pending > 0 && matches!(op, b"BMC" | b"BDC" | b"EMC") {
                    return Err(unsafe_cut(opener));
                }
                if let Some(span) = shown
                    && matches!(op, b"Tj" | b"TJ")
                {
                    return Err(SpliceError::Unsafe(span));
                }
                if moves_text(op) || matches!(op, b"BT" | b"ET") {
                    shown = None;
                }
            }
        }
        match op {
            b"BT" => in_text = true,
            b"ET" => in_text = false,
            op if makes_path(op) => in_path = true,
            op if ends_path(op) => in_path = false,
            _ => {}
        }
    }
}

/// Whether white-space and comments `gap` end inside a comment.
fn in_comment(gap: &[u8]) -> bool {
    gap.iter().fold(false, |comment, &c| match c {
        b'%' => true,
        b'\r' | b'\n' => false,
        _ => comment,
    })
}

/// `content`'s streams with `cuts` taken out and `rewrites` made, none of
/// them in a cut: a stream neither touches is borrowed as it is.
fn written<'a>(content: &'a Concat, cuts: &[Cut], rewrites: &[&Rewrite]) -> Vec<Cow<'a, [u8]>> {
    (0..content.stream_count())
        .map(|i| cut(content, i, cuts, rewrites))
        .collect()
}

/// Stream `i` of `content` with `cuts` taken out and `rewrites` made.
/// Whether a cut needs a space in its place is decided by the bytes kept
/// either side of it, as a neighbouring cut may take the bytes next to it
/// too; a list written again starts and ends with a delimiter, so needs
/// none.
fn cut<'a>(content: &'a Concat, i: usize, cuts: &[Cut], rewrites: &[&Rewrite]) -> Cow<'a, [u8]> {
    let s = content.bytes();
    let stream = content.range(i);
    let mut edits: Vec<(&Range<usize>, &[u8])> = (cuts.iter())
        .map(|c| (&c.range, &[][..]))
        .chain(rewrites.iter().map(|r| (&r.range, &r.with[..])))
        .filter(|(r, _)| r.start < stream.end && r.end > stream.start)
        .collect();
    edits.sort_by_key(|(r, _)| r.start);
    let mut out: Option<Vec<u8>> = None;
    let mut at = stream.start;
    let mut edits = edits.into_iter().peekable();
    while let Some((range, with)) = edits.next() {
        let out = out.get_or_insert_with(|| Vec::with_capacity(stream.len()));
        out.extend_from_slice(&s[at..range.start.max(stream.start)]);
        out.extend_from_slice(with);
        at = range.end.min(stream.end);
        // A list written again needs no space; when the next byte is taken
        // too, the cut after it decides.
        if !with.is_empty() || edits.peek().is_some_and(|(r, _)| r.start == at) {
            continue;
        }
        if at < stream.end
            && out.last().is_some_and(|&c| !lex::is_space(c))
            && !lex::is_space(s[at])
        {
            out.push(b' ');
        }
    }
    match out {
        None => Cow::Borrowed(&s[stream]),
        Some(mut out) => {
            out.extend_from_slice(&s[at..stream.end]);
            Cow::Owned(out)
        }
    }
}
