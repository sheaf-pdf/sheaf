//! The dumps' pdf-gfx arm (M18 slice 23, test code): every corpus page
//! read by pdf-gfx's interpreter and encoder through the door, its
//! records written as sheaf's interpreter's arm writes them, by the
//! fields both name, so the two dumps set side by side say where the two
//! interpreters read a page otherwise. `SHEAF_DUMP_INTERP=gfx` picks it.
//!
//! A page is read through [`Reasoned`], a source's objects with the
//! reasons one that doesn't read gives ([`why`]): through [`Objects`],
//! which gives none until the interpreter is pdf-gfx's, no object would
//! read as damaged or capped. Each [`Fault::Damaged`] an interpretation
//! that draws forms is told of is counted ([`Counted`]), and fails
//! nothing: the events dump's last column. A text bag is read as
//! sheaf's [`text_bag`](crate::content::text_bag) reads one, its body
//! copied onto pdf-gfx's events ([`text_bag`]), each form judged on its
//! own ([`Forms::Skip`]) and told of nothing (`&mut ()`).

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::ops::ControlFlow;
use std::path::Path;

use super::{EventRecords, GlyphRecord, Records, Rewritten, glyph_records, show_records};
use super::{
    LIM, SUMMARY_COLUMNS, cap_number, join_number, mode_number, paint_number, rule_number,
};
use crate::content::{Concat, Fonts, PageContent, ReadObjects, TextBag};
use crate::forms::corpus::{Corpus, Entry, each_entry};
use crate::pdf::content::Operators;
use crate::pdf::fault::{Place, Sink};
use crate::pdf::gfx::{
    Cap, EncodedPage, Error, Event, Fault, Forms, ImageOf, Kind, Mark, MarkProps, Matrix, Opts,
    Rect, Refused, Seg, State, Summary, Unwritable, Wants, encode_with, interpret_with,
};
use crate::pdf::{Limit, Object, ObjectId, Resolve, StreamExt, Streams, Unreadable, Why};
use crate::source::Limits;
use crate::store::{BlobKind, Store};

/// Why `error`, a source's failure to read an object, is: the reason
/// pdf-gfx is told. The source's I/O is [`Why::Source`]; a password or
/// decryption that fails, [`Why::Locked`]; a reader's cap,
/// [`Why::Capped`]; anything else, the file's, [`Why::Damaged`].
pub(in crate::content) fn why(error: &crate::pdf::Error) -> Why {
    use crate::pdf::Error as E;
    match error {
        E::IO(_) => Why::Source,
        E::InvalidPassword | E::Decryption(_) => Why::Locked,
        E::Limit(_) | E::ReferenceLimit => Why::Capped,
        _ => Why::Damaged,
    }
}

thread_local! {
    /// Objects [`Reasoned`] read where [`ReadObjects::peek`] lent
    /// nothing, on this thread: each read as none, and counted.
    static LENT_NOTHING: Cell<usize> = const { Cell::new(0) };
}

/// How many objects [`Reasoned`] has read on this thread where
/// [`ReadObjects::peek`] lent nothing.
pub(in crate::content) fn lent_nothing() -> usize {
    LENT_NOTHING.with(Cell::get)
}

/// A source's objects as [`Objects`](crate::content::Objects) gives them,
/// and why one that doesn't read doesn't: [`Resolve::try_resolve`] gives
/// what [`ReadObjects::peek`] lends, and where it lends nothing asks the
/// source ([`ReadObjects::read`], which keeps nothing): an object that
/// isn't there is none, any other failure its reason ([`why`]). One the
/// source reads where `peek` lent nothing is none, and counted
/// ([`lent_nothing`]). A stream is decoded as `Objects` decodes it, past
/// its limit [`Why::Limit`], and any other failure its reason.
pub(in crate::content) struct Reasoned<'a, R: ?Sized>(pub &'a R);

impl<R: ?Sized> Clone for Reasoned<'_, R> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<R: ?Sized> Copy for Reasoned<'_, R> {}

impl<'a, R: ReadObjects + ?Sized> Resolve<'a> for Reasoned<'a, R> {
    fn resolve(&self, id: ObjectId) -> Option<&'a Object> {
        self.0.peek(id)
    }

    fn try_resolve(&self, id: ObjectId) -> Result<Option<&'a Object>, Unreadable> {
        if let Some(object) = self.0.peek(id) {
            return Ok(Some(object));
        }
        use crate::pdf::Error as E;
        match self.0.read(id, |_| ()) {
            Ok(()) => {
                LENT_NOTHING.with(|n| n.set(n.get() + 1));
                Ok(None)
            }
            Err(E::MissingXrefEntry | E::ObjectNotFound(_)) => Ok(None),
            Err(error) => Err(Unreadable::new(id, why(&error), error.to_string())),
        }
    }
}

impl<R: ReadObjects + ?Sized> Streams for Reasoned<'_, R> {
    fn decoded(&self, id: ObjectId, limit: usize) -> Result<Cow<'_, [u8]>, Unreadable> {
        let read = self.0.read(id, |object| {
            let stream = object.as_stream()?;
            Some(stream.decompressed_content_with_limit(limit))
        });
        match read {
            Ok(Some(Ok(data))) => Ok(Cow::Owned(data)),
            Ok(Some(Err(crate::pdf::Error::Limit(Limit::Decompressed)))) => {
                Err(Unreadable::new(id, Why::Limit, "decodes past the limit"))
            }
            Ok(Some(Err(error))) | Err(error) => {
                Err(Unreadable::new(id, why(&error), error.to_string()))
            }
            Ok(None) => Err(Unreadable::new(id, Why::Damaged, "not a stream")),
        }
    }
}

/// A sink that counts each object it's told is damaged, and fails
/// nothing: what sheaf's sink would fail an interpretation on.
#[derive(Debug, Default)]
pub(in crate::content) struct Counted {
    pub damaged: usize,
}

impl Sink<Fault> for Counted {
    fn fault(&mut self, _: Place, fault: Fault) {
        if matches!(fault, Fault::Damaged { .. }) {
            self.damaged += 1;
        }
    }
}

/// What [`text_bag`] reads of content, as sheaf's `text_bag` reads it:
/// the strings shown, and the operators that end a word.
pub(in crate::content) const TEXT_BAG_WANTS: Wants = Wants {
    ops: Operators::of(&[b"BT", b"ET", b"Td", b"TD", b"Tm", b"T*", b"'", b"\""]),
    text: true,
    ..Wants::NONE
};

/// [`text_bag`](crate::content::text_bag)'s body on pdf-gfx's events:
/// the words `content`, drawn with `resources`, draws in `fonts`, each
/// form it draws judged on its own ([`Forms::Skip`]), told of nothing.
pub(in crate::content) fn text_bag<'o, O>(
    objects: O,
    content: &Concat,
    resources: Option<&'o Object>,
    lim: &Limits,
    fonts: &Fonts,
) -> Result<TextBag, Error>
where
    O: Resolve<'o> + Streams + Copy,
{
    let mut bag = TextBag::default();
    let mut run = String::new();
    let opts = Opts {
        forms: Forms::Skip,
        wants: TEXT_BAG_WANTS,
    };
    let lim = lim.gfx();
    interpret_with(
        objects,
        content,
        resources,
        &lim,
        fonts,
        opts,
        &mut (),
        |event| {
            match event.kind {
                // One that ends a word.
                Kind::Op => flush(&mut bag, &mut run),
                Kind::Text(show) => {
                    let font = show.font.filter(|_| !show.stock);
                    match font.and_then(|f| f.text(show.string)) {
                        Some(text) => run.push_str(&text),
                        None => {
                            flush(&mut bag, &mut run);
                            bag.add_hash(sheaf_util::checksum(show.string));
                        }
                    }
                }
                _ => {}
            }
            ControlFlow::Continue(())
        },
    )?;
    flush(&mut bag, &mut run);
    Ok(bag)
}

/// The words of `run`, text run together, counted in `bag`, and `run`
/// emptied; an empty one has none.
fn flush(bag: &mut TextBag, run: &mut String) {
    if !run.is_empty() {
        bag.add(&std::mem::take(run));
    }
}

/// The glyph dump's records of what `content`, drawn with `resources`
/// from `objects`' objects in `fonts`, shows, forms drawn ([`Reasoned`],
/// [`Counted`]): each string, and each glyph, counted in `glyphs`.
pub(super) fn shown<R: ReadObjects>(
    objects: &R,
    content: &Concat,
    resources: Option<&Object>,
    fonts: &Fonts,
    records: &mut Records,
    glyphs: &mut usize,
) -> Result<(), String> {
    let read = interpret_with(
        Reasoned(objects),
        content,
        resources,
        &LIM.gfx(),
        fonts,
        Opts::default(),
        &mut Counted::default(),
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

fn matrix(m: Matrix) -> [f32; 6] {
    [m.a, m.b, m.c, m.d, m.e, m.f]
}

fn rect(r: Rect) -> [f32; 4] {
    [r.x0, r.y0, r.x1, r.y1]
}

/// A mark's record, as the sheaf arm writes one.
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

/// The state an event is in, as the sheaf arm writes it: what both
/// interpreters keep.
fn state_records(r: &mut EventRecords, state: &State) {
    r.reals("ctm", &matrix(state.ctm));
    r.reals("line_width", &[state.line_width]);
    r.number("cap", cap_number(state.line_cap));
    r.number("join", join_number(state.line_join));
    r.reals("miter", &[state.miter_limit]);
    r.maybe_reals(
        "clip",
        state.clip_bounds().map(rect).as_ref().map(|r| &r[..]),
    );
    let text = &state.text;
    r.number("stock", u64::from(text.stock));
    r.reals("size", &[text.size]);
    r.reals("spacing", &[text.char_space, text.word_space]);
    r.reals("scale", &[text.scale]);
    r.undrawn(|r| r.reals("leading", &[text.leading]));
    r.reals("rise", &[text.rise]);
    r.number("mode", mode_number(text.mode));
    r.reals("matrix", &matrix(text.matrix));
    r.reals("line", &[text.line.x, text.line.y]);
    r.reals("pen", &[text.pen.x, text.pen.y]);
}

/// `event`'s record, as the sheaf arm writes one. A kind of event sheaf's
/// interpreter has no like of is a record of its own (`other`).
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
            r.drawn_only(show.string, show.font.and_then(crate::content::Font::id));
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

/// The class of what stopped an interpretation, by pdf-gfx's error, in
/// the sheaf arm's names where sheaf's interpreter had the like: content
/// that doesn't lex is `lex`, a form whose stream doesn't decode
/// `xobject`, a cap `limit:<cap>`, geometry past an `f32` `not-finite`,
/// an operator that can't be written again `unwritable:<why>`; and an
/// object that doesn't read for a reason that isn't the file's
/// `unreadable:<why>`, a page stream that wasn't decoded `page-stream`,
/// an inline image sized under a cap `capped-inline`. Anything else is
/// `other`, a finding.
pub(in crate::content) fn class(e: &Error) -> String {
    match e {
        Error::Content { .. } => "lex".into(),
        Error::FormStream { .. } => "xobject".into(),
        Error::Limit(cap) => format!("limit:{}", cap_name(*cap)),
        Error::NotFinite => "not-finite".into(),
        Error::Unwritable { why, .. } => format!("unwritable:{}", unwritable_name(*why)),
        Error::Unreadable(unreadable) => format!("unreadable:{}", why_name(unreadable.why)),
        Error::PageStream(_) => "page-stream".into(),
        Error::CappedInlineImage { .. } => "capped-inline".into(),
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
/// refusals as [`class`] names them, and an interpretation in the
/// encoder that stopped where the page's didn't as `encode:<class>`, as
/// the sheaf arm names one.
fn encode_class(e: &Error) -> String {
    match e {
        Error::Unwritable { .. } | Error::PageStream(_) | Error::CappedInlineImage { .. } => {
            class(e)
        }
        _ => format!("encode:{}", class(e)),
    }
}

/// A summary's fields, in the sheaf arm's order, tab-separated.
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
    let columns = fields.map(|n| n.to_string());
    debug_assert_eq!(columns.len(), SUMMARY_COLUMNS);
    columns.join("\t")
}

/// `content`, drawn with `resources` from `objects`' objects, forms drawn
/// and every event wanted, each event's record written to `records`;
/// what it was told of damaged objects counted in `sink`.
fn recorded<R: ReadObjects>(
    objects: &R,
    content: &Concat,
    resources: Option<&Object>,
    fonts: &Fonts,
    records: &mut EventRecords,
    sink: &mut Counted,
) -> Result<Summary, Error> {
    interpret_with(
        Reasoned(objects),
        content,
        resources,
        &LIM.gfx(),
        fonts,
        Opts::default(),
        sink,
        |event| {
            event_records(records, &event);
            ControlFlow::Continue(())
        },
    )
}

/// Whether `encoded`, written again by pdf-gfx, interpreted again with
/// each form written again in its place, draws what the content did, as
/// the sheaf arm's `drawn_again` asks it.
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
    let mut sink = Counted::default();
    let read = recorded(&again, &content, resources, fonts, &mut records, &mut sink);
    let (_, redrawn) = records.finish(title);
    read.is_ok() && redrawn == drawn
}

/// The events dump's columns for one page, as the sheaf arm's
/// `page_events` gives them, read by pdf-gfx: its last column how many
/// times the interpretation was told an object is damaged.
pub(in crate::content) fn page_events<R: ReadObjects>(
    objects: &R,
    content: &Concat,
    resources: Option<&Object>,
    fonts: &Fonts,
    (at, full): (&str, bool),
) -> String {
    let mut records = EventRecords::new(full);
    let mut sink = Counted::default();
    let lent = lent_nothing();
    let read = recorded(objects, content, resources, fonts, &mut records, &mut sink);
    let (events, drawn) = records.finish(&format!("{at}: events"));
    let summary = match &read {
        Ok(summary) => summary_columns(summary),
        Err(_) => ["-"; SUMMARY_COLUMNS].join("\t"),
    };
    let (encoding, why, redrawn) = match read {
        Err(e) => ("-".to_string(), class(&e), "-"),
        Ok(_) => {
            let encoded = encode_with(
                Reasoned(objects),
                content,
                resources,
                &LIM.gfx(),
                fonts,
                &mut Counted::default(),
            );
            match encoded {
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
                    let same =
                        drawn_again(objects, &encoded, resources, fonts, drawn, (&title, full));
                    let same = if same { "same" } else { "differs" };
                    (format!("{:032x}", sum.finish()), "ok".to_string(), same)
                }
            }
        }
    };
    let lent = lent_nothing() - lent;
    if lent > 0 {
        eprintln!("{at}: {lent} objects read where peek lent none");
    }
    let damaged = sink.damaged;
    format!("{summary}\t{events:032x}\t{encoding}\t{why}\t{redrawn}\t{damaged}")
}

/// A file of corpus pages, a `path\tpage` line each, that
/// [`the_two_arms_set_side_by_side`] reads by both interpreters.
const COMPARE: &str = "SHEAF_DUMP_COMPARE";

/// Where [`the_two_arms_set_side_by_side`] writes its lines.
const COMPARE_OUT: &str = "SHEAF_DUMP_COMPARE_OUT";

/// Where `a` and `b`, records in full a line each, part: `same`, or how
/// many lines part and the kinds of record they are (`path:3,text:1`,
/// where both have as many; `-` where one has more), the first's index,
/// how many each has, and the two lines, each cut to 600 characters.
fn first_parting(a: &str, b: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    let cut = |s: &str| {
        let s = s.replace('\t', " ");
        match s.char_indices().nth(600) {
            Some((at, _)) => format!("{}…", &s[..at]),
            None => s,
        }
    };
    let at = (a.iter().zip(&b)).position(|(x, y)| x != y);
    let at = match at {
        Some(at) => at,
        None if a.len() == b.len() => return "same".into(),
        None => a.len().min(b.len()),
    };
    let line = |l: &[&str]| l.get(at).map_or("<end>".to_string(), |s| cut(s));
    let kinds = match a.len() == b.len() {
        false => "-".to_string(),
        true => {
            let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
            for (x, _) in a.iter().zip(&b).filter(|(x, y)| x != y) {
                *kinds.entry(x.split(' ').next().unwrap_or("")).or_default() += 1;
            }
            let kinds: Vec<String> = kinds.iter().map(|(k, n)| format!("{k}:{n}")).collect();
            kinds.join(",")
        }
    };
    let (n, m) = (a.len(), b.len());
    format!("{kinds} {at}/{n}/{m}: {} || {}", line(&a), line(&b))
}

/// The records in full of what `records` was written with, taken.
fn full_events(records: &mut EventRecords) -> String {
    records.full.take().unwrap_or_default()
}

fn full_records(records: &mut Records) -> String {
    records.full.take().unwrap_or_default()
}

/// One page's line of [`the_two_arms_set_side_by_side`]: the file and the
/// page, then for its events, what it shows and its text bag, where the
/// two arms' records part ([`first_parting`]), each arm's class first.
fn compare_page<R: ReadObjects>(
    objects: &R,
    content: &Concat,
    resources: Option<&Object>,
    (path, page): (&str, usize),
) -> String {
    let fonts = Fonts::default();
    let for_text = Fonts::for_text();

    let mut a = EventRecords::new(true);
    let read_a = super::recorded(objects, content, resources, &fonts, &mut a);
    let class_a = read_a
        .as_ref()
        .map_or_else(super::interp_class, |_| "ok".into());
    let mut b = EventRecords::new(true);
    let mut sink = Counted::default();
    let read_b = recorded(objects, content, resources, &fonts, &mut b, &mut sink);
    let class_b = read_b.as_ref().map_or_else(class, |_| "ok".into());
    let events = first_parting(&full_events(&mut a), &full_events(&mut b));

    let mut a = Records::new(true);
    let _ = super::shown(objects, content, resources, &fonts, &mut a, &mut 0);
    let mut b = Records::new(true);
    let _ = shown(objects, content, resources, &fonts, &mut b, &mut 0);
    let glyphs = first_parting(&full_records(&mut a), &full_records(&mut b));

    let mut a = Records::new(true);
    let bag_a = super::text_bag(objects, content, resources, LIM, &for_text);
    if let Ok(bag) = &bag_a {
        super::bag_records(&mut a, bag);
    }
    let mut b = Records::new(true);
    let bag_b = text_bag(Reasoned(objects), content, resources, LIM, &for_text);
    if let Ok(bag) = &bag_b {
        super::bag_records(&mut b, bag);
    }
    let bag_class_a = bag_a
        .as_ref()
        .map_or_else(super::interp_class, |_| "ok".into());
    let bag_class_b = bag_b.as_ref().map_or_else(class, |_| "ok".into());
    let bags = first_parting(&full_records(&mut a), &full_records(&mut b));
    format!(
        "{path}\t{page}\t{class_a}\t{class_b}\t{}\t{events}\t{glyphs}\t{bag_class_a}\t{bag_class_b}\t{bags}",
        sink.damaged
    )
}

/// Each page [`COMPARE`] lists, read by both interpreters with every
/// record in full, the line [`compare_page`] gives written to
/// [`COMPARE_OUT`]: where the two dumps differ on a page, the record
/// where its two readings part, to class the difference by.
#[test]
#[ignore = "both arms' records of the pages $SHEAF_DUMP_COMPARE lists, where they part, to $SHEAF_DUMP_COMPARE_OUT"]
fn the_two_arms_set_side_by_side() {
    let Some(list) = std::env::var_os(COMPARE) else {
        eprintln!("skipped: {COMPARE} isn't set");
        return;
    };
    let out = std::env::var_os(COMPARE_OUT).expect("SHEAF_DUMP_COMPARE_OUT");
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let list = std::fs::read_to_string(&list).expect("the list");
    let mut pages: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for line in list.lines().filter(|l| !l.is_empty()) {
        let (path, page) = line.split_once('\t').expect("path\tpage");
        let page = page.trim().parse().expect("a page number");
        pages.entry(path.to_string()).or_default().push(page);
    }
    let entries: Vec<Entry> = (pages.keys())
        .map(|path| Entry {
            path: path.clone(),
            ..Entry::default()
        })
        .collect();
    let lines = each_entry(&entries, |entry| {
        let path = entry.path.as_str();
        let Ok(bytes) = std::fs::read(corpus.root.join(path)) else {
            return vec![format!("{path}\t-\tunopened")];
        };
        let kind = BlobKind::file(Path::new(path), None);
        let Ok(src) = Store::new().open(bytes.into(), kind) else {
            return vec![format!("{path}\t-\tunopened")];
        };
        let all = src.pages();
        let mut lines = Vec::new();
        for &n in &pages[path] {
            let Some(page) = n.checked_sub(1).and_then(|i| all.get(i)) else {
                lines.push(format!("{path}\t{n}\tno such page"));
                continue;
            };
            let Ok(content) = PageContent::read(src.pdf(), &page.contents, LIM) else {
                lines.push(format!("{path}\t{n}\tunread"));
                continue;
            };
            let resources = page.resources.as_deref();
            let read = || compare_page(src.pdf(), content.concat(), resources, (path, n));
            let line = std::panic::catch_unwind(std::panic::AssertUnwindSafe(read));
            lines.push(line.unwrap_or_else(|_| format!("{path}\t{n}\tpanicked")));
        }
        lines
    });
    let mut text = lines.into_iter().flatten().collect::<Vec<_>>().join("\n");
    text.push('\n');
    std::fs::write(&out, text).expect("the output");
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::content::fixture;
    use crate::pdf::{Document, LazyPdf, Stream, dictionary};

    /// A file with `objects` (each written as it's given, numbered from 1)
    /// and a catalog, as a source reads it, each dictionary that has the
    /// key `broken` made what no parser reads, its length kept.
    fn source(objects: &[Object], broken: &[u8]) -> LazyPdf {
        let mut doc = Document::new();
        for object in objects {
            doc.add_object(object.clone());
        }
        let pages = doc.add_object(dictionary! {
            "Type" => Object::name("Pages"),
            "Kids" => Vec::<Object>::new(),
            "Count" => 0i64,
        });
        let catalog = doc.add_object(dictionary! {
            "Type" => Object::name("Catalog"),
            "Pages" => pages,
        });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let key = [b"/".as_slice(), broken].concat();
        let mut from = 0;
        while let Some(at) = (bytes[from..].windows(key.len())).position(|w| w == key) {
            let at = from + at;
            let open = (bytes[..at].windows(2))
                .rposition(|w| w == b"<<")
                .expect("its dictionary");
            bytes[open..open + 2].copy_from_slice(b"]]");
            from = at + key.len();
        }
        LazyPdf::open(Arc::<[u8]>::from(bytes), None, &Limits::DEFAULT.reader).unwrap()
    }

    /// [`Reasoned`] gives each object as `peek` lends it, and each that
    /// doesn't read its reason: one that isn't there is none, one that
    /// doesn't parse is damaged, and a chain of references past the
    /// reader's depth is capped. A stream decodes as `Objects` decodes it,
    /// past its limit [`Why::Limit`], a filter that fails damaged.
    #[test]
    fn a_reasoned_source_gives_each_reason() {
        let depth = Limits::DEFAULT.reader.max_depth;
        // 1: a dictionary, 2: one to break, 3..: a chain past the depth,
        // then two streams.
        let mut objects = vec![
            Object::from(dictionary! { "Marker" => 7i64 }),
            Object::from(dictionary! { "Broken" => 1i64 }),
        ];
        let chain = depth + 2;
        for n in 0..chain {
            objects.push(Object::from(ObjectId::new(4 + n as u32, 0)));
        }
        objects.push(Object::from(dictionary! { "End" => 1i64 }));
        let plain = ObjectId::new(objects.len() as u32 + 1, 0);
        objects.push(Object::from(Stream::new(
            dictionary! {},
            b"0123456789".to_vec(),
        )));
        let filtered = ObjectId::new(objects.len() as u32 + 1, 0);
        let mut bad = Stream::new(dictionary! {}, b"not flate".to_vec());
        bad.dict.set("Filter", Object::name("FlateDecode"));
        objects.push(Object::from(bad));
        let pdf = source(&objects, b"Broken");
        let objects = Reasoned(&pdf);
        let reason = |id: ObjectId| {
            objects
                .try_resolve(id)
                .map(|o| o.is_some())
                .map_err(|e| e.why)
        };

        assert_eq!(reason(ObjectId::new(1, 0)), Ok(true));
        assert_eq!(reason(ObjectId::new(9999, 0)), Ok(false));
        assert_eq!(reason(ObjectId::new(2, 0)), Err(Why::Damaged));
        assert_eq!(reason(ObjectId::new(3, 0)), Err(Why::Capped));
        // Within the depth, the chain is followed.
        assert_eq!(reason(ObjectId::new(3 + chain as u32 - 2, 0)), Ok(true));
        assert_eq!(objects.resolve(ObjectId::new(3, 0)), None);
        assert_eq!(lent_nothing(), 0);

        let decoded = |id, limit| {
            objects
                .decoded(id, limit)
                .map(|d| d.len())
                .map_err(|e| e.why)
        };
        assert_eq!(decoded(plain, 100), Ok(10));
        assert_eq!(decoded(plain, 4), Err(Why::Limit));
        assert_eq!(decoded(filtered, 100), Err(Why::Damaged));
        assert_eq!(decoded(ObjectId::new(1, 0), 100), Err(Why::Damaged));
        assert_eq!(decoded(ObjectId::new(3, 0), 100), Err(Why::Capped));
    }

    /// What the dumps' pdf-gfx arm counts can be counted: an ExtGState
    /// drawn that doesn't parse is told to the sink as damaged, once, and
    /// the page read as though it weren't there; through [`Objects`],
    /// which gives no reason, nothing is told. One behind a chain past
    /// the reader's depth fails the page, capped, sink or none, where
    /// `Objects` reads it as none.
    #[test]
    fn a_damaged_object_drawn_is_told_and_a_capped_one_fails_the_page() {
        let depth = Limits::DEFAULT.reader.max_depth;
        let mut objects = vec![Object::from(dictionary! { "Broken" => 1i64, "LW" => 3i64 })];
        let chain = depth + 2;
        for n in 0..chain {
            objects.push(Object::from(ObjectId::new(3 + n as u32, 0)));
        }
        objects.push(Object::from(dictionary! { "LW" => 5i64 }));
        let pdf = source(&objects, b"Broken");
        let resources = Object::from(dictionary! {
            "ExtGState" => dictionary! {
                "Bad" => ObjectId::new(1, 0),
                "Deep" => ObjectId::new(2, 0),
            },
        });
        let fonts = Fonts::default();
        let lim = Limits::DEFAULT.gfx();
        let run = |objects, content: &[u8], sink: &mut Counted| {
            let content = Concat::new(&[content]);
            let (resources, opts) = (Some(&resources), Opts::default());
            let continues = |_: Event<'_>| ControlFlow::Continue(());
            match objects {
                true => interpret_with(
                    Reasoned(&pdf),
                    &content,
                    resources,
                    &lim,
                    &fonts,
                    opts,
                    sink,
                    continues,
                ),
                false => interpret_with(
                    crate::content::Objects(&pdf),
                    &content,
                    resources,
                    &lim,
                    &fonts,
                    opts,
                    sink,
                    continues,
                ),
            }
        };
        let mut told = Counted::default();
        assert!(run(true, b"/Bad gs 0 0 m 1 1 l S", &mut told).is_ok());
        assert_eq!(told.damaged, 1);
        let mut told = Counted::default();
        assert!(run(false, b"/Bad gs 0 0 m 1 1 l S", &mut told).is_ok());
        assert_eq!(told.damaged, 0);

        let deep = run(true, b"/Deep gs", &mut Counted::default());
        assert!(
            matches!(&deep, Err(Error::Unreadable(u)) if u.why == Why::Capped),
            "{deep:?}"
        );
        assert_eq!(class(&deep.unwrap_err()), "unreadable:capped");
        let mut none = ();
        let deep = interpret_with(
            Reasoned(&pdf),
            &Concat::new(&[b"/Deep gs"]),
            Some(&resources),
            &lim,
            &fonts,
            Opts::default(),
            &mut none,
            |_| ControlFlow::Continue(()),
        );
        assert!(matches!(deep, Err(Error::Unreadable(_))), "{deep:?}");
        assert!(run(false, b"/Deep gs", &mut Counted::default()).is_ok());
    }

    /// Each reason a source's error gives is the one [`why`] says.
    #[test]
    fn each_source_error_is_its_reason() {
        use crate::pdf::{DecryptionError, Error as E};
        let io = || E::IO(std::io::Error::other("gone"));
        let cases = [
            (io(), Why::Source),
            (E::InvalidPassword, Why::Locked),
            (E::Decryption(DecryptionError::Padding), Why::Locked),
            (E::Limit(Limit::Depth), Why::Capped),
            (E::Limit(Limit::Decompressed), Why::Capped),
            (E::ReferenceLimit, Why::Capped),
            (E::MissingXrefEntry, Why::Damaged),
            (E::ObjectIdMismatch, Why::Damaged),
            (E::IndirectObject { offset: 3 }, Why::Damaged),
            (E::InvalidStream("bad".into()), Why::Damaged),
            (E::Syntax("bad".into()), Why::Damaged),
        ];
        for (error, reason) in cases {
            assert_eq!(why(&error), reason, "{error}");
        }
    }

    /// Every fixture page's pdf-gfx events line says it was read, written
    /// again and, written again, drawn the same, with nothing damaged, and
    /// says the same twice.
    #[test]
    fn each_fixture_s_events_by_pdf_gfx_are_drawn_the_same_written_again() {
        let fonts = Fonts::default();
        for (name, f) in fixture::every() {
            let content = f.content();
            let at = (name, false);
            let line = page_events(&f.pdf, content.concat(), f.resources(), &fonts, at);
            let last: Vec<&str> = line.rsplit('\t').take(3).collect();
            assert_eq!(last, ["0", "same", "ok"], "{name}: {line}");
            let again = page_events(&f.pdf, content.concat(), f.resources(), &fonts, at);
            assert_eq!(again, line, "{name}");
        }
    }
}
