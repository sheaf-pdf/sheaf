//! The content interpreter (M11 slice 3): what content draws, an operator
//! at a time, in the graphics and text state it's drawn in, as PDFium
//! interprets it (`CPDF_StreamContentParser`), so that where it says a
//! glyph is, PDFium draws it. It reads the toolkit's tokens
//! ([`lex`](super::lex)) and the fonts files draw in
//! ([`Foreign`](super::Foreign)), and hands each operator, each string
//! shown and each form drawn to a visitor, as an [`Event`]:
//!
//! - every operator, inline images among them, with where it is and its
//!   operands as written ([`At`]), in the state before it ([`State`]):
//!   `cm`, `q` and `Q`, `w` and `gs` (`/LW`, `/Font`), and the text state
//!   and position, `Tc`, `Tw`, `Tz`, `TL`, `Ts`, `Tr`, `Tf`, `BT`, `Tm`,
//!   `Td`, `TD`, `T*`, `'` and `"`;
//! - each string `Tj`, `TJ`, `'` and `"` show, with its glyphs and the box
//!   each spans ([`TextShow`], [`Glyph`]);
//! - each use of a Form XObject, interpreted in a state of its own between
//!   a [`Kind::FormBegin`] and a [`Kind::FormEnd`] ([`FormUse`], [`UseId`]),
//!   or none, each XObject's `Do` an operator like another
//!   ([`Forms::Report`]); a use too deep refused and passed over, as
//!   PDFium passes it over, and a use within itself refused at once,
//!   where PDFium draws it to that depth ([`Refused`]);
//! - each path painted or made the clip, in PageSpace ([`PathDrawn`],
//!   [`Seg`]); each image, an XObject or inline ([`ImageDrawn`]); and each
//!   shading `sh` paints ([`ShadingDrawn`]);
//! - the clip each is drawn in ([`State::clip`]): paths `W` and `W*` make,
//!   text shown in a clipping mode, and the `/BBox` of each form drawn
//!   ([`Clip`]);
//! - marked content: each range opened and ended, each point, and the
//!   ranges every event is in ([`Mark`], [`Marks`]), with their `/MCID`,
//!   `/ActualText` and optional content, and an XObject's own `/OC`.
//!
//! A visitor that reads less says what it reads ([`Wants`]), and isn't
//! handed the rest.
//!
//! Where PDFium reads content its own way, so does this: an operand that
//! isn't a number is 0, a `Q` with nothing saved is passed over, text in
//! a font the resources haven't got is drawn in PDFium's stock Helvetica,
//! a form's resources are its own, else those of the content that draws
//! it, a category they haven't got looked up in the page's, paths are
//! built as PDFium builds them, and marked content that doesn't balance
//! is read as PDFium reads it ([`Marks`]).
//!
//! What it reads can be written again ([`encode`]): a page's operators,
//! and each form's it draws, as written, but those that move text to a
//! line, written where the interpreter says the line is.
//!
//! Content is untrusted, so its work is bounded ([`ContentLimits`]): how
//! deep forms are drawn, the states `q` saves, marked content's nesting,
//! the operators, glyphs, path segments and bytes of content a page's
//! content interprets across every use of its forms, and what its forms
//! and fonts decompress to. Past a cap is an error, never a partial
//! answer, and so is geometry an `f32` can't hold; it never panics or
//! recurses past `form_depth`.
//!
//! [`ContentLimits`]: crate::source::ContentLimits

mod clip;
mod encode;
#[cfg(test)]
mod encode_tests;
mod marked;
mod op;
mod path;
mod state;
#[cfg(test)]
mod tests;
mod text;
mod xobject;

use std::collections::{HashMap, HashSet};
use std::ops::{ControlFlow, Range};
use std::rc::Rc;

use lopdf::{Object, ObjectId};

pub use clip::{Clip, ClipPart};
pub use encode::{EncodeError, Encoded, Unwritable, encode};
pub use marked::{Mark, Marks};
pub use op::Ops;
pub use path::{PathDrawn, Seg};
pub use state::{State, TextState};
pub use text::{Glyph, Quad, TextShow};
pub use xobject::{FormUse, ImageDrawn, ImageOf, Refused, UseId};

use super::lex::{self, LexError, ResCx, Step, Tok};
use super::page::{Concat, ReadObjects, direct};
use super::{FontId, FontKind, FontScope, Fonts, Paint, Props, TooLarge};
use crate::geom::Rect;
use crate::source::Limits;
use op::Op;
use path::Builder;
use state::{number, nums};
use xobject::{Form, ImageFacts, XObject};

/// How the forms content draws are interpreted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Forms {
    /// Each use in turn, where it's drawn.
    #[default]
    Descend,
    /// None: a `Do` is an operator like another, and the XObject it
    /// names isn't read, as where each is judged on its own (the removal
    /// check). Inline images are drawn as ever.
    Report,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Opts {
    pub forms: Forms,
    /// What the visitor reads: everything, unless it says.
    pub wants: Wants,
}

/// What of an interpretation its visitor reads ([`Opts::wants`]): what it
/// doesn't isn't handed it, nor built for it. What it does is as where it
/// wants everything, and so is what interpreting returns, its [`Summary`]
/// and its errors, the caps and the geometry an `f32` can't hold among
/// them: what isn't wanted is still worked out where those depend on it.
/// The events of forms, images, shadings and marked content are always
/// handed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wants {
    /// The operators handed it as a [`Kind::Op`]; the others are
    /// interpreted unseen.
    pub ops: Ops,
    /// Each string shown, [`Kind::Text`].
    pub text: bool,
    /// Each path painted or made the clip, [`Kind::Path`].
    pub paths: bool,
}

impl Wants {
    pub const ALL: Wants = Wants {
        ops: Ops::ALL,
        text: true,
        paths: true,
    };
    pub const NONE: Wants = Wants {
        ops: Ops::NONE,
        text: false,
        paths: false,
    };
}

impl Default for Wants {
    fn default() -> Wants {
        Wants::ALL
    }
}

/// What content does at one place, in the state it's done in.
#[derive(Debug)]
pub struct Event<'e> {
    pub at: At<'e>,
    /// The state before the operator: for a string shown, where it
    /// starts, `'` and `"` having moved to the next line; for a path, the
    /// clip before the one it makes.
    pub state: &'e State,
    /// The marked ranges it's in, the innermost first, across the forms
    /// it's drawn in: for a range begun or ended, those outside it.
    pub marks: &'e Marks,
    pub kind: Kind<'e>,
}

#[derive(Debug)]
pub enum Kind<'e> {
    /// An operator, or an inline image: every one, before what it does.
    Op,
    /// A string shown, after its operator's [`Kind::Op`].
    Text(TextShow<'e>),
    /// A form drawn, after its `Do`'s [`Kind::Op`]: the events of its
    /// content follow, and a [`Kind::FormEnd`].
    FormBegin(FormUse<'e>),
    FormEnd(FormUse<'e>),
    /// A form that isn't drawn here.
    FormRefused {
        form: ObjectId,
        why: Refused,
    },
    /// A path painted, or made the clip, after its painting operator's
    /// [`Kind::Op`].
    Path(PathDrawn<'e>),
    /// An image drawn, after its `Do`'s or its inline image's
    /// [`Kind::Op`].
    Image(ImageDrawn),
    /// A shading `sh` paints, after its [`Kind::Op`].
    Shading(ShadingDrawn),
    /// An XObject a `Do` names that PDFium doesn't draw: a PostScript one,
    /// or one of a subtype it doesn't know.
    Ignored(ObjectId),
    /// A marked range opened, after its `BMC`'s or `BDC`'s [`Kind::Op`]:
    /// the events of what's in it follow, and a [`Kind::MarkedEnd`].
    MarkedBegin(&'e Mark),
    /// A marked range ended: by an `EMC`, after its [`Kind::Op`], or where
    /// the content it's in ends.
    MarkedEnd(&'e Mark),
    /// A marked point, `MP` or `DP`, after its [`Kind::Op`].
    MarkedPoint(&'e Mark),
}

/// What a shading `sh` paints: what the clip leaves, within the
/// shading's `/BBox`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadingDrawn {
    /// The shading, where the resources name it by reference.
    pub shading: Option<ObjectId>,
    /// A box that holds what it paints: the clip's
    /// ([`State::clip_bounds`]), within its `/BBox` through the CTM;
    /// None where neither bounds it, and it paints all the page shows.
    pub bounds: Option<Rect>,
}

/// Where an event is: an operator, or an inline image, in the content
/// interpreted or in a form's. A range that content leaves open ends
/// where the content does: at an empty operator at its end.
#[derive(Debug, Clone)]
pub struct At<'e> {
    /// The form use whose content it's in; None in the content
    /// interpreted.
    pub stream: Option<&'e UseId>,
    pub tok: Tok<'e>,
    /// Where `tok` is in its content.
    pub op: Range<usize>,
    content: &'e [u8],
    operands: &'e [Range<usize>],
}

impl<'e> At<'e> {
    /// The bytes of the content it's in.
    pub fn content(&self) -> &'e [u8] {
        self.content
    }

    /// Where it is, from its first operand.
    pub fn range(&self) -> Range<usize> {
        self.operands.first().map_or(self.op.start, |r| r.start)..self.op.end
    }

    /// Its operands as written, in order.
    pub fn operands(&self) -> impl Iterator<Item = &'e [u8]> + '_ {
        self.operands.iter().map(|r| &self.content[r.clone()])
    }

    /// Operand `i` from the last, as PDFium takes them: 0 is the last.
    pub fn operand(&self, i: usize) -> Option<&'e [u8]> {
        let r = self.operands.get(self.operands.len().checked_sub(i + 1)?)?;
        Some(&self.content[r.clone()])
    }

    /// It as the lexer's step.
    fn step(&self) -> Step<'e, 'e> {
        Step {
            s: self.content,
            tok: self.tok.clone(),
            at: self.op.clone(),
            operands: self.operands,
        }
    }
}

/// What an interpretation did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    /// Operators, inline images among them, in the content and in every
    /// use of its forms.
    pub operators: usize,
    /// Glyphs shown in a font.
    pub glyphs: usize,
    /// Bytes of content lexed: the content's, and each form use's.
    pub interpreted: usize,
    /// Bytes taken from the page's budget
    /// ([`decompressed`](crate::source::ContentLimits::decompressed)): the
    /// content's, each form's data once, what each font picked
    /// decompresses to ([`FontScope::font`]), and what drawing its
    /// program's glyphs cost: a simple font's as it's read, a CIDFont's
    /// for each glyph shown, once
    /// ([`Foreign::charge_outlines`](super::Foreign::charge_outlines)).
    pub decompressed: usize,
    /// Path segments painted or made the clip.
    pub segments: usize,
    /// Form uses interpreted.
    pub uses: usize,
    /// Form uses refused.
    pub refused: usize,
    /// How deep in forms it went: 1 where the content draws a form that
    /// draws none.
    pub deepest_form: usize,
    /// The most states saved at once, across the content and the forms
    /// it's drawing.
    pub deepest_q: usize,
    /// How deep marked content nested, within the content or a form's.
    pub deepest_marked: usize,
    /// Whether the visitor stopped it.
    pub stopped: bool,
}

/// Content that couldn't be interpreted.
#[derive(Debug, thiserror::Error)]
pub enum InterpError {
    /// Content that can't be lexed: the content interpreted, or form
    /// `form`'s, `error`'s offset into it.
    #[error("{error}{}", .form.map_or(String::new(), |f| format!(" in form {} {} R", f.0, f.1)))]
    Lex {
        form: Option<ObjectId>,
        error: LexError,
    },
    /// An XObject drawn that can't be read: a form's stream, or the
    /// object.
    #[error("XObject {} {} R can't be read: {error}", .xobject.0, .xobject.1)]
    XObject {
        xobject: ObjectId,
        error: lopdf::Error,
    },
    #[error("past the cap on {0}")]
    Limit(Cap),
    /// A place, a box or a matrix past what an `f32` holds: a CTM, a
    /// form's matrix or its `/BBox` through it, a line's start or the
    /// pen, a glyph's box, a path's points or what it paints, an image's
    /// corners or what a shading paints. PDFium's geometry is no better
    /// there, and another reader's may be.
    #[error("geometry past what an f32 holds")]
    NotFinite,
}

/// A cap on the work of one interpretation
/// ([`ContentLimits`](crate::source::ContentLimits)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Cap {
    #[error("operators interpreted")]
    Operators,
    #[error("glyphs shown")]
    Glyphs,
    /// Bytes of content lexed, a form's each time it's drawn.
    #[error("bytes of content interpreted")]
    Interpreted,
    /// Path segments: of one path being built, and of all painted or
    /// made the clip.
    #[error("path segments")]
    Segments,
    #[error("graphics states saved")]
    States,
    /// Marked content nested within one content: past
    /// [`depth`](crate::source::ContentLimits::depth).
    #[error("marked content nested")]
    Marked,
    /// What forms and fonts decompress to, and drawing their programs'
    /// glyphs costs, with the content: what's left of the page's
    /// [`decompressed`](crate::source::ContentLimits::decompressed).
    #[error("what the forms and fonts drawn decompress to")]
    Decompressed,
}

/// Interprets `content`, drawn with `resources` (a page's or a form's
/// `/Resources` as written), its fonts read through `fonts`, a source's,
/// handing each event to `visit` in order, until the end or until `visit`
/// breaks.
pub fn interpret<R: ReadObjects>(
    objects: &R,
    content: &Concat,
    resources: Option<&Object>,
    lim: &Limits,
    fonts: &Fonts,
    opts: Opts,
    mut visit: impl FnMut(Event<'_>) -> ControlFlow<()>,
) -> Result<Summary, InterpError> {
    let res = ResCx::read(objects, resources);
    let mut run = Run {
        objects,
        lim,
        page: resources,
        page_res: &res,
        fonts: fonts.scope(),
        opts,
        visit: &mut visit,
        summary: Summary::default(),
        saved: 0,
        path: Vec::new(),
        chain: Vec::new(),
        xobjects: HashMap::new(),
        outlines: HashSet::new(),
        budget: (lim.content.decompressed).saturating_sub(content.bytes().len()),
    };
    let top = Frame {
        content: content.bytes(),
        stream: None,
        resources,
        owner: PAGE,
        res: &res,
    };
    match run
        .stream(&top, State::default(), Marks::default())
        .map_err(|e| e.within(None))
    {
        Ok(()) => {}
        Err(Halt::Break) => run.summary.stopped = true,
        Err(Halt::Fail(e)) => return Err(e),
    }
    run.summary.decompressed = lim.content.decompressed - run.budget;
    Ok(run.summary)
}

/// Whose resources the content interpreted draws with, where fonts are
/// picked by name ([`FontScope`]): no object is numbered 0.
const PAGE: ObjectId = (0, 0);

/// One interpretation.
struct Run<'a, R: ReadObjects> {
    objects: &'a R,
    lim: &'a Limits,
    /// The resources of the content interpreted.
    page: Option<&'a Object>,
    page_res: &'a ResCx,
    fonts: FontScope<'a>,
    opts: Opts,
    visit: &'a mut dyn FnMut(Event<'_>) -> ControlFlow<()>,
    summary: Summary,
    /// States saved and not restored, in all the content being
    /// interpreted.
    saved: usize,
    /// The `Do`s of the forms being drawn, and the forms.
    path: Vec<u32>,
    chain: Vec<ObjectId>,
    /// XObjects read, each once.
    xobjects: HashMap<ObjectId, Rc<Drawn>>,
    /// The CIDFont glyphs whose outlines' drawing is charged
    /// ([`Foreign::charge_outlines`](super::Foreign::charge_outlines)),
    /// each once.
    outlines: HashSet<(FontId, u32)>,
    /// What the forms and fonts may yet decompress to, and drawing the
    /// glyphs of their programs cost.
    budget: usize,
}

/// An XObject, as drawing it needs it.
enum Drawn {
    Form {
        form: Form,
        /// Where its own resources have color spaces, what lexing it
        /// needs of them.
        res: Option<Box<ResCx>>,
    },
    Image(ImageFacts),
    Other,
    None,
}

/// Content being interpreted.
struct Frame<'f> {
    content: &'f [u8],
    stream: Option<&'f UseId>,
    /// The resources it draws with: its own, or those of the content that
    /// draws it.
    resources: Option<&'f Object>,
    /// Whose they are: [`PAGE`], or a form's.
    owner: ObjectId,
    res: &'f ResCx,
}

/// What interpreting content keeps that's its own, and goes with it: the
/// states its `q`s saved, the path it's building, the marked ranges it
/// opened, and the text it showed in a clipping mode since its last
/// `ET`.
struct Local {
    state: State,
    saved: Vec<State>,
    path: Builder,
    /// The ranges open: its own, inside those it's drawn in.
    marks: Marks,
    /// How many of them are its own.
    own: usize,
    clip_glyphs: Vec<Quad>,
    /// The strings shown, by one operator each, that drew them.
    clip_texts: usize,
}

/// Why interpreting stops early.
enum Stop {
    Lex(LexError),
    Halt(Halt),
}

enum Halt {
    Break,
    Fail(InterpError),
}

impl From<LexError> for Stop {
    fn from(e: LexError) -> Stop {
        Stop::Lex(e)
    }
}

impl Stop {
    /// It, from the content of form `form`, or of the content
    /// interpreted.
    fn within(self, form: Option<ObjectId>) -> Halt {
        match self {
            Stop::Lex(error) => Halt::Fail(InterpError::Lex { form, error }),
            Stop::Halt(halt) => halt,
        }
    }
}

fn past(cap: Cap) -> Stop {
    Stop::Halt(Halt::Fail(InterpError::Limit(cap)))
}

fn not_finite() -> Stop {
    Stop::Halt(Halt::Fail(InterpError::NotFinite))
}

impl<'a, R: ReadObjects> Run<'a, R> {
    /// `frame`'s content, from `state`, within the marked ranges `marks`:
    /// what it saves, builds and opens is its own, and goes with it, the
    /// ranges it leaves open ending where it does.
    fn stream(&mut self, frame: &Frame<'_>, state: State, marks: Marks) -> Result<(), Stop> {
        self.summary.interpreted += frame.content.len();
        if self.summary.interpreted > self.lim.content.interpreted {
            return Err(past(Cap::Interpreted));
        }
        let mut local = Local {
            state,
            saved: Vec::new(),
            path: Builder::default(),
            marks,
            own: 0,
            clip_glyphs: Vec::new(),
            clip_texts: 0,
        };
        let done = lex::steps(frame.content, frame.res, self.lim, |step| {
            self.step(frame, &mut local, &step)
        });
        self.saved -= local.saved.len();
        done?;
        let end = frame.content.len();
        let at = At {
            stream: frame.stream,
            tok: Tok::Op(b""),
            op: end..end,
            content: frame.content,
            operands: &[],
        };
        while local.own > 0 {
            local.own -= 1;
            if let Some(closed) = local.marks.pop() {
                self.emit(
                    &at,
                    &local.state,
                    &local.marks,
                    Kind::MarkedEnd(closed.mark()),
                )?;
            }
        }
        Ok(())
    }

    fn step(
        &mut self,
        frame: &Frame<'_>,
        local: &mut Local,
        step: &Step<'_, '_>,
    ) -> Result<(), Stop> {
        self.summary.operators += 1;
        if self.summary.operators > self.lim.content.operators {
            return Err(past(Cap::Operators));
        }
        let at = At {
            stream: frame.stream,
            tok: step.tok.clone(),
            op: step.at.clone(),
            content: frame.content,
            operands: step.operands,
        };
        let op = match step.tok {
            Tok::Op(name) => Op::of(name),
            Tok::InlineImage(_) | Tok::Operand(_) => Op::Other,
        };
        if self.opts.wants.ops.has(op) {
            self.emit(&at, &local.state, &local.marks, Kind::Op)?;
        }
        if op.builds_path() {
            local.path.build(op, step);
            return match local.path.len() > self.lim.content.segments {
                true => Err(past(Cap::Segments)),
                false => Ok(()),
            };
        }
        // Each step but a path operator ends PDFium's own reading of them.
        local.path.interrupt();
        let depth = self.lim.content.depth;
        match op {
            // What moves the CTM or the line may move it past an `f32`.
            Op::Transform | Op::MoveLine | Op::MoveLineLeading | Op::NextLine => {
                local.state.set(op, step);
                if !local.state.is_finite() {
                    return Err(not_finite());
                }
            }
            Op::LineWidth
            | Op::LineCap
            | Op::LineJoin
            | Op::MiterLimit
            | Op::BeginText
            | Op::TextMatrix
            | Op::CharSpace
            | Op::WordSpace
            | Op::Scale
            | Op::Leading
            | Op::Rise
            | Op::RenderMode => local.state.set(op, step),
            Op::Paint(paint) => return self.paint(local, &at, paint),
            Op::Save => {
                if self.saved >= self.lim.content.states {
                    return Err(past(Cap::States));
                }
                local.saved.push(local.state.clone());
                self.saved += 1;
                self.summary.deepest_q = self.summary.deepest_q.max(self.saved);
            }
            // A `Q` with nothing saved is passed over, as in PDFium.
            Op::Restore => {
                if let Some(restored) = local.saved.pop() {
                    local.state = restored;
                    self.saved -= 1;
                }
            }
            Op::Font => {
                let name = step.arg(1).and_then(|o| string(o, depth));
                self.pick(frame, &mut local.state, name.as_deref().unwrap_or_default())?;
                if let Some([size]) = nums(step) {
                    local.state.text.size = size;
                }
            }
            Op::ExtGState => self.ext_gstate(frame, &mut local.state, step)?,
            Op::Show => self.show(local, &at, step.arg(0))?,
            Op::NextLineShow | Op::NextLineShowSpaced => {
                local.state.next_line_of(op, step);
                if !local.state.is_finite() {
                    return Err(not_finite());
                }
                self.show(local, &at, step.arg(0))?;
            }
            Op::ShowArray => self.show_array(local, &at, step.arg(0))?,
            Op::EndText => end_text(local),
            Op::Draw => self.draw(frame, local, &at, step)?,
            Op::Shade => self.shade(frame, local, &at, step)?,
            Op::BeginMarked | Op::BeginMarkedProps => {
                self.begin_marked(frame, local, &at, step, op)?
            }
            Op::EndMarked => self.end_marked(local, &at)?,
            Op::Point | Op::PointProps => self.point(frame, local, &at, step, op)?,
            Op::Other => {
                if let Tok::InlineImage(ref data) = step.tok {
                    return self.inline_image(local, &at, data.clone());
                }
            }
            // Built above.
            Op::Move
            | Op::Line
            | Op::Curve
            | Op::CurveFromPen
            | Op::CurveToEnd
            | Op::ClosePath
            | Op::Rect
            | Op::Clip(_) => {}
        }
        Ok(())
    }

    fn emit(
        &mut self,
        at: &At<'_>,
        state: &State,
        marks: &Marks,
        kind: Kind<'_>,
    ) -> Result<(), Stop> {
        hand(self.visit, at, state, marks, kind)
    }

    /// Font `name` picked, as PDFium's `FindFont` picks one: the
    /// resources' font by that name, else the stock Helvetica. What its
    /// streams decompress to comes out of the page's budget.
    fn pick(&mut self, frame: &Frame<'_>, state: &mut State, name: &[u8]) -> Result<(), Stop> {
        let objects = self.objects;
        let font = match self.holder(frame, b"Font") {
            Some((owner, resources)) => self
                .fonts
                .font(
                    objects,
                    owner,
                    Some(resources),
                    name,
                    self.lim,
                    &mut self.budget,
                )
                .map_err(|_: TooLarge| past(Cap::Decompressed))?,
            None => None,
        };
        let text = &mut state.text;
        text.stock = font.is_none();
        text.font = Some(font.unwrap_or_else(|| self.fonts.stock()));
        Ok(())
    }

    /// `gs`: the ExtGState's line width, caps, joins and miter limit, and
    /// its font, as PDFium reads them (`CPDF_AllStates::ProcessExtGS`):
    /// its `/Font`'s first element is a font's name in the resources,
    /// which a reference to a font isn't, so a font given by reference is
    /// the stock one.
    fn ext_gstate(
        &mut self,
        frame: &Frame<'_>,
        state: &mut State,
        step: &Step<'_, '_>,
    ) -> Result<(), Stop> {
        let objects = self.objects;
        let Some(name) = step.arg(0).and_then(|o| string(o, self.lim.content.depth)) else {
            return Ok(());
        };
        let gs = self
            .holder(frame, b"ExtGState")
            .and_then(|(_, resources)| entry(objects, resources, b"ExtGState", &name))
            .and_then(|gs| direct(objects, gs)?.as_dict().ok());
        let Some(gs) = gs else {
            return Ok(());
        };
        let value = |key: &[u8]| direct(objects, gs.get(key).ok()?);
        let float = |o: &Object| direct(objects, o)?.as_float().ok().map(|n| n as f32);
        let finite = |key: &[u8]| value(key).and_then(float).filter(|n| n.is_finite());
        if let Some(width) = finite(b"LW") {
            state.line_width = width;
        }
        if let Some(n) = finite(b"LC") {
            state.line_cap = state::cap(state::int(n));
        }
        if let Some(n) = finite(b"LJ") {
            state.line_join = state::join(state::int(n));
        }
        if let Some(limit) = finite(b"ML") {
            state.miter_limit = limit;
        }
        if let Some(Object::Array(font)) = value(b"Font") {
            let name = match font.first().and_then(|n| direct(objects, n)) {
                Some(Object::Name(n)) => n.as_slice(),
                Some(Object::String(s, _)) => s.as_slice(),
                _ => b"",
            };
            self.pick(frame, state, name)?;
            let size = font.get(1).and_then(float).unwrap_or(0.0);
            if size.is_finite() {
                state.text.size = size;
            }
        }
        Ok(())
    }

    /// `Tj`, `'` or `"`: the string `operand` is, where it's one.
    fn show(&mut self, local: &mut Local, at: &At<'_>, operand: Option<&[u8]>) -> Result<(), Stop> {
        let Some(string) = operand.and_then(|o| string(o, self.lim.content.depth)) else {
            return Ok(());
        };
        if string.is_empty() {
            return Ok(());
        }
        self.text(local, at, &[(string, 0.0)], 0.0)
    }

    /// `TJ`: the strings of the array `operand` is, and the adjustments
    /// between them, as PDFium reads them
    /// (`Handle_ShowText_Positioning`): an empty string is passed over,
    /// its adjustments added to the string's before it, but where one
    /// was, the last string's adjustment is lost, as PDFium looks for it
    /// past the strings it kept; an array of no strings moves the pen
    /// along the line, in a font or none.
    fn show_array(
        &mut self,
        local: &mut Local,
        at: &At<'_>,
        operand: Option<&[u8]>,
    ) -> Result<(), Stop> {
        let Some(array) = operand.filter(|o| o.first() == Some(&b'[')) else {
            return Ok(());
        };
        let depth = self.lim.content.depth;
        let items = lex::elements(array, depth);
        let is_string =
            |i: &[u8]| matches!(i, [b'(', ..]) || matches!(i, [b'<', c, ..] if *c != b'<');
        let adjust = |i: &[u8]| Some(number(i)).filter(|n| n.is_finite() && *n != 0.0);
        if !items.iter().any(|i| is_string(i)) {
            let text = &mut local.state.text;
            for n in items.into_iter().filter_map(adjust) {
                text.pen.x -= n * text.size / 1000.0 * text.scale;
            }
            return match text.pen.x.is_finite() {
                true => Ok(()),
                false => Err(not_finite()),
            };
        }
        let mut strings: Vec<(Vec<u8>, f32)> = Vec::new();
        let mut first = 0.0;
        let mut passed_over = false;
        for item in items {
            if !is_string(item) {
                let n = adjust(item).unwrap_or(0.0);
                match strings.last_mut() {
                    Some((_, after)) => *after += n,
                    None => first += n,
                }
                continue;
            }
            match string(item, depth).filter(|s| !s.is_empty()) {
                Some(string) => strings.push((string, 0.0)),
                None => passed_over = true,
            }
        }
        if passed_over && let Some((_, after)) = strings.last_mut() {
            *after = 0.0;
        }
        self.text(local, at, &strings, first)
    }

    /// `strings` shown, each followed by its adjustment, `first` before
    /// them, as PDFium's `AddTextObject` shows them: one text object,
    /// where the pen is once `first` moves it, each string on from the
    /// one before it and its adjustment (in text space before `Tz`), the
    /// pen moved past them all, then by the last string's adjustment. In
    /// no font, none is drawn and the pen stays where it is. In a
    /// clipping mode, what they draw is kept for the clip `ET` makes, but
    /// not in a Type3 font, which PDFium only fills, nor past the text a
    /// clip takes, which cuts nothing. Geometry that isn't finite fails
    /// it ([`InterpError::NotFinite`]).
    fn text(
        &mut self,
        local: &mut Local,
        at: &At<'_>,
        strings: &[(Vec<u8>, f32)],
        first: f32,
    ) -> Result<(), Stop> {
        let wanted = self.opts.wants.text;
        let id = local.state.text.font;
        let Some(font) = id.map(|id| self.fonts.get(id)) else {
            if wanted {
                for (string, _) in strings {
                    let show = TextShow::new(string, &local.state, None, 0.0);
                    hand(self.visit, at, &local.state, &local.marks, Kind::Text(show))?;
                }
            }
            return Ok(());
        };
        let vertical = font.is_vertical();
        // Down the column, or along the line through `Tz`.
        let adjust = |state: &mut State, n: f32| {
            let text = &mut state.text;
            match vertical {
                true => text.pen.y -= n * text.size / 1000.0,
                false => text.pen.x -= n * text.size / 1000.0 * text.scale,
            }
        };
        let clips = font.kind() != FontKind::Type3 && local.state.text.mode.clips();
        // Where each string shown moves the pen to is guessed.
        let guessed = font.approx().codes || font.approx().widths;
        if clips && !strings.is_empty() {
            local.clip_texts += 1;
        }
        adjust(&mut local.state, first);
        let mut run = 0.0;
        for (i, (string, after)) in strings.iter().enumerate() {
            let (end, glyphs) = text::advance(font, &local.state.text, string, run);
            self.summary.glyphs += glyphs;
            if self.summary.glyphs > self.lim.content.glyphs {
                return Err(past(Cap::Glyphs));
            }
            let outlines = &mut self.outlines;
            let first = |glyph| id.is_some_and(|id| outlines.insert((id, glyph)));
            font.charge_outlines(string, first, &mut self.budget)
                .map_err(|_: TooLarge| past(Cap::Decompressed))?;
            let show = TextShow::new(string, &local.state, Some(font), run);
            if !show.bounded(font, glyphs, end) {
                return Err(not_finite());
            }
            if clips && local.clip_texts <= clip::TEXTS {
                local.clip_glyphs.extend(show.glyphs().map(|g| g.quad));
            }
            if wanted {
                hand(self.visit, at, &local.state, &local.marks, Kind::Text(show))?;
            }
            let text = &mut local.state.text;
            text.guessed = text.guessed || guessed || font.width_guessed_in(string);
            run = end;
            if i + 1 < strings.len() {
                run -= after * text.size / 1000.0;
            }
        }
        let text = &mut local.state.text;
        match vertical {
            true => text.pen.y += run,
            false => text.pen.x += run * text.scale,
        }
        if let Some((_, after)) = strings.last() {
            adjust(&mut local.state, *after);
        }
        match local.state.is_finite() {
            true => Ok(()),
            false => Err(not_finite()),
        }
    }

    /// A painting operator: the path it ends painted or made the clip,
    /// where PDFium does either.
    fn paint(&mut self, local: &mut Local, at: &At<'_>, paint: Paint) -> Result<(), Stop> {
        let Some(ended) = local.path.end(paint, &local.state) else {
            return Ok(());
        };
        self.summary.segments += ended.segs.len();
        if self.summary.segments > self.lim.content.segments {
            return Err(past(Cap::Segments));
        }
        if !(path::finite(ended.segs) && finite_rect(ended.bounds)) {
            return Err(not_finite());
        }
        if self.opts.wants.paths {
            let drawn = PathDrawn {
                segments: ended.segs,
                paint: ended.paint,
                clip: ended.clip,
                bounds: ended.bounds,
            };
            self.emit(at, &local.state, &local.marks, Kind::Path(drawn))?;
        }
        if let Some(rule) = ended.clip {
            local.state.clip = local.state.clip.with_path(ended.segs, rule, ended.points);
        }
        Ok(())
    }

    /// An inline image: an image mask where its `/IM` says so.
    fn inline_image(&mut self, local: &Local, at: &At<'_>, data: Range<usize>) -> Result<(), Stop> {
        let depth = self.lim.content.depth;
        let mask = lex::image_entry(at.content, at.op.clone(), depth, &[b"IM", b"ImageMask"]);
        let facts = ImageFacts {
            stencil: mask.and_then(|m| lex::object(m, depth)) == Some(Object::Boolean(true)),
            ..ImageFacts::default()
        };
        let image = ImageDrawn::new(ImageOf::Inline { data }, &facts, local.state.ctm);
        if !image.quad.is_finite() {
            return Err(not_finite());
        }
        self.emit(at, &local.state, &local.marks, Kind::Image(image))
    }

    /// `Do`: the form it names drawn, or the image, where it names one;
    /// an XObject of another subtype is reported and not drawn. Where
    /// XObjects are judged on their own ([`Forms::Report`]), none is read.
    fn draw(
        &mut self,
        frame: &Frame<'_>,
        local: &Local,
        at: &At<'_>,
        step: &Step<'_, '_>,
    ) -> Result<(), Stop> {
        let objects = self.objects;
        if self.opts.forms == Forms::Report {
            return Ok(());
        }
        let Some(name) = step.arg(0).and_then(|o| string(o, self.lim.content.depth)) else {
            return Ok(());
        };
        let id = self
            .holder(frame, b"XObject")
            .and_then(|(_, resources)| entry(objects, resources, b"XObject", &name))
            .and_then(|x| x.as_reference().ok());
        let Some(id) = id else {
            return Ok(());
        };
        let (state, marks) = (&local.state, &local.marks);
        let drawn = self.xobject(id)?;
        let (form, res) = match &*drawn {
            Drawn::Form { form, res } => (form, res),
            Drawn::Image(facts) => {
                let image = ImageDrawn::new(ImageOf::XObject(id), facts, state.ctm);
                if !image.quad.is_finite() {
                    return Err(not_finite());
                }
                return self.emit(at, state, marks, Kind::Image(image));
            }
            Drawn::Other => return self.emit(at, state, marks, Kind::Ignored(id)),
            Drawn::None => return Ok(()),
        };
        let why = if self.chain.contains(&id) {
            Some(Refused::Cycle)
        } else if self.chain.len() >= self.lim.content.form_depth {
            Some(Refused::Depth)
        } else {
            None
        };
        if let Some(why) = why {
            self.summary.refused += 1;
            return self.emit(at, state, marks, Kind::FormRefused { form: id, why });
        }
        let matrix = form.matrix.then(state.ctm);
        let clip = form.bbox.map(|bbox| matrix.bounds(bbox));
        if !(state::finite(matrix) && clip.is_none_or(finite_rect)) {
            return Err(not_finite());
        }
        self.path.push(at.op.start as u32);
        self.chain.push(id);
        self.summary.uses += 1;
        self.summary.deepest_form = self.summary.deepest_form.max(self.chain.len());
        let use_id = UseId::new(&self.path);
        let shown = FormUse {
            form: id,
            id: &use_id,
            matrix,
            bbox: form.bbox,
            oc: form.oc,
        };
        // Its own resources, else those of what draws it; color spaces its
        // own haven't got are the page's.
        let (resources, owner, res) = match &form.resources {
            Some(own) => {
                let res = res.as_deref().unwrap_or(self.page_res);
                (Some(own), id, res)
            }
            None => (frame.resources, frame.owner, frame.res),
        };
        let inner = Frame {
            content: &form.data,
            stream: Some(&use_id),
            resources,
            owner,
            res,
        };
        let drawn_in = self
            .emit(at, state, marks, Kind::FormBegin(shown))
            .and_then(|()| {
                let begun = state.form(shown.matrix, shown.bbox);
                self.stream(&inner, begun, marks.clone())
            })
            .map_err(|e| Stop::Halt(e.within(Some(id))));
        self.path.pop();
        self.chain.pop();
        drawn_in?;
        self.emit(at, state, marks, Kind::FormEnd(shown))
    }

    /// XObject `id`, read once.
    fn xobject(&mut self, id: ObjectId) -> Result<Rc<Drawn>, Stop> {
        if let Some(known) = self.xobjects.get(&id) {
            return Ok(known.clone());
        }
        let read = XObject::read(self.objects, id, self.budget)
            .map_err(|error| Stop::Halt(Halt::Fail(InterpError::XObject { xobject: id, error })))?;
        let drawn = match read {
            XObject::Form(form) => {
                self.budget -= form.data.len();
                let res = form
                    .resources
                    .as_ref()
                    .filter(|r| category(self.objects, r, b"ColorSpace").is_some())
                    .map(|r| Box::new(ResCx::read(self.objects, Some(r))));
                Drawn::Form { form, res }
            }
            XObject::Image(facts) => Drawn::Image(facts),
            XObject::Other => Drawn::Other,
            XObject::None => Drawn::None,
            XObject::TooLarge => return Err(past(Cap::Decompressed)),
        };
        let drawn = Rc::new(drawn);
        self.xobjects.insert(id, drawn.clone());
        Ok(drawn)
    }

    /// `sh`: the shading it names painted, where the resources have it.
    fn shade(
        &mut self,
        frame: &Frame<'_>,
        local: &Local,
        at: &At<'_>,
        step: &Step<'_, '_>,
    ) -> Result<(), Stop> {
        let objects = self.objects;
        let Some(name) = step.arg(0).and_then(|o| string(o, self.lim.content.depth)) else {
            return Ok(());
        };
        let Some(written) = self
            .holder(frame, b"Shading")
            .and_then(|(_, resources)| entry(objects, resources, b"Shading", &name))
        else {
            return Ok(());
        };
        let dict = match direct(objects, written) {
            Some(Object::Dictionary(dict)) => dict,
            Some(Object::Stream(stream)) => &stream.dict,
            _ => return Ok(()),
        };
        // PDFium paints a shading within its `/BBox`, and nothing where
        // that isn't four numbers.
        let bbox = dict.has(b"BBox").then(|| {
            let n = xobject::array(objects, dict, b"BBox", 4).unwrap_or_else(|| vec![0.0; 4]);
            local.state.ctm.bounds(Rect::new(n[0], n[1], n[2], n[3]))
        });
        let bounds = match (local.state.clip_bounds(), bbox) {
            (Some(clip), Some(bbox)) => Some(clip.intersect(bbox)),
            (clip, bbox) => clip.or(bbox),
        };
        if !bounds.is_none_or(finite_rect) {
            return Err(not_finite());
        }
        let shading = ShadingDrawn {
            shading: written.as_reference().ok(),
            bounds,
        };
        self.emit(at, &local.state, &local.marks, Kind::Shading(shading))
    }

    /// `BMC` or `BDC`: a marked range opened, where PDFium opens one: a
    /// `BDC` opens none whose list is a name the resources haven't got, or
    /// neither a name nor a dictionary.
    fn begin_marked(
        &mut self,
        frame: &Frame<'_>,
        local: &mut Local,
        at: &At<'_>,
        step: &Step<'_, '_>,
        op: Op,
    ) -> Result<(), Stop> {
        let depth = self.lim.content.depth;
        let tag = |i: usize| {
            step.arg(i)
                .and_then(|t| string(t, depth))
                .unwrap_or_default()
        };
        let mark = match op {
            Op::BeginMarkedProps => match step.arg(0).and_then(|list| self.props(frame, list)) {
                Some((Props::Named(_, None), _)) | None => return Ok(()),
                Some((props, named)) => {
                    Mark::new(tag(1), Some(props), named, frame.stream, at.range())
                }
            },
            _ => Mark::new(tag(0), None, None, frame.stream, at.range()),
        };
        if local.own >= depth {
            return Err(past(Cap::Marked));
        }
        self.emit(at, &local.state, &local.marks, Kind::MarkedBegin(&mark))?;
        local.marks.push(mark);
        local.own += 1;
        self.summary.deepest_marked = self.summary.deepest_marked.max(local.own);
        Ok(())
    }

    /// `EMC`: the innermost range content opened ended; one with nothing
    /// open is passed over, as in PDFium.
    fn end_marked(&mut self, local: &mut Local, at: &At<'_>) -> Result<(), Stop> {
        if local.own == 0 {
            return Ok(());
        }
        local.own -= 1;
        let Some(closed) = local.marks.pop() else {
            return Ok(());
        };
        self.emit(
            at,
            &local.state,
            &local.marks,
            Kind::MarkedEnd(closed.mark()),
        )
    }

    /// `MP` or `DP`: a marked point, which PDFium passes over.
    fn point(
        &mut self,
        frame: &Frame<'_>,
        local: &Local,
        at: &At<'_>,
        step: &Step<'_, '_>,
        op: Op,
    ) -> Result<(), Stop> {
        let depth = self.lim.content.depth;
        let tag = |i: usize| {
            step.arg(i)
                .and_then(|t| string(t, depth))
                .unwrap_or_default()
        };
        let mark = match op {
            Op::PointProps => match step.arg(0).and_then(|list| self.props(frame, list)) {
                Some((props, named)) => {
                    Mark::new(tag(1), Some(props), named, frame.stream, at.range())
                }
                None => Mark::new(tag(1), None, None, frame.stream, at.range()),
            },
            _ => Mark::new(tag(0), None, None, frame.stream, at.range()),
        };
        self.emit(at, &local.state, &local.marks, Kind::MarkedPoint(&mark))
    }

    /// The property list `operand` is: one written in the content, or one
    /// named, and the resources' `/Properties` entry by that name as
    /// written, as [`content::marked`](crate::content::marked) reads one;
    /// None for what's neither a name nor a dictionary.
    fn props<'x>(&self, frame: &Frame<'x>, operand: &[u8]) -> Option<(Props, Option<&'x Object>)>
    where
        'a: 'x,
    {
        let Some(name) = lex::name(operand) else {
            return match lex::object(operand, self.lim.content.depth)? {
                Object::Dictionary(dict) => Some((Props::Inline(dict), None)),
                _ => None,
            };
        };
        let objects = self.objects;
        let written = self
            .holder(frame, b"Properties")
            .and_then(|(_, resources)| entry(objects, resources, b"Properties", &name));
        let list = written.and_then(|w| direct(objects, w)?.as_dict().ok());
        Some((Props::Named(name.into_owned(), list.cloned()), written))
    }

    /// The resources a category of `frame`'s is looked up in, and whose
    /// they are, as PDFium looks one up (`FindResourceHolder`): its own,
    /// where they have a dictionary of it, else the page's.
    fn holder<'x>(&self, frame: &Frame<'x>, key: &[u8]) -> Option<(ObjectId, &'x Object)>
    where
        'a: 'x,
    {
        let objects = self.objects;
        let has = |r: &&Object| category(objects, r, key).is_some();
        match frame.resources.filter(has) {
            Some(own) => Some((frame.owner, own)),
            None if frame.owner != PAGE => self.page.filter(has).map(|page| (PAGE, page)),
            None => None,
        }
    }
}

/// `kind`, at `at` in `state` and `marks`, handed to `visit`: what
/// [`Run::emit`] does, for where a font of the run's is borrowed.
fn hand(
    visit: &mut dyn FnMut(Event<'_>) -> ControlFlow<()>,
    at: &At<'_>,
    state: &State,
    marks: &Marks,
    kind: Kind<'_>,
) -> Result<(), Stop> {
    let event = Event {
        at: at.clone(),
        state,
        marks,
        kind,
    };
    match visit(event) {
        ControlFlow::Continue(()) => Ok(()),
        ControlFlow::Break(()) => Err(Stop::Halt(Halt::Break)),
    }
}

/// Whether `r`'s sides are all finite.
fn finite_rect(r: Rect) -> bool {
    [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite())
}

/// `ET`: the text shown in a clipping mode since the last one cuts the
/// clip, where the mode is still one, as in PDFium.
fn end_text(local: &mut Local) {
    let glyphs = std::mem::take(&mut local.clip_glyphs);
    let texts = std::mem::take(&mut local.clip_texts);
    if texts > 0 && local.state.text.mode.clips() {
        local.state.clip = local.state.clip.with_text(glyphs, texts);
    }
}

/// `resources`'s dictionary of `key`, where it has one.
fn category<'x>(
    objects: &'x impl ReadObjects,
    resources: &'x Object,
    key: &[u8],
) -> Option<&'x lopdf::Dictionary> {
    let resources = direct(objects, resources)?.as_dict().ok()?;
    direct(objects, resources.get(key).ok()?)?.as_dict().ok()
}

/// Resource `name` of category `key` in `resources`, as written.
fn entry<'x>(
    objects: &'x impl ReadObjects,
    resources: &'x Object,
    key: &[u8],
    name: &[u8],
) -> Option<&'x Object> {
    category(objects, resources, key)?.get(name).ok()
}

/// The string an operand is, as PDFium takes one (`GetString`): a
/// string's bytes, its escapes read, or a name's, its own decoded.
fn string(operand: &[u8], depth: usize) -> Option<Vec<u8>> {
    match operand {
        [b'/', ..] => Some(lex::name(operand)?.into_owned()),
        [b'(', ..] | [b'<', ..] => match lex::object(operand, depth)? {
            Object::String(s, _) => Some(s),
            _ => None,
        },
        _ => None,
    }
}
