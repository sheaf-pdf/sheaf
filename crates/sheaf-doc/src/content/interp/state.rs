//! The graphics and text state content is drawn in, as PDFium keeps it
//! (`CPDF_AllStates`), and what the operators that only set it do.
//! Operands are read as PDFium reads them: an operand that isn't a number,
//! or isn't there, is 0, and a number that isn't finite leaves the state
//! as it was.

use super::clip::Clip;
use super::op::Op;
use super::path::Seg;
use crate::annot::Pt;
use crate::content::lex::Step;
use crate::content::{FillRule, FontId, LineCap, LineJoin, TextMode};
use crate::geom::{Affine, Rect};

/// The graphics state where an operator is: what `q` saves and `Q`
/// restores, which in PDFium holds the text matrix and where text goes
/// next too.
#[derive(Debug, Clone)]
pub struct State {
    /// User space onto PageSpace: every `cm` before, and the `/Matrix` of
    /// each form it's drawn in.
    pub ctm: Affine,
    /// `w`, or an ExtGState's `/LW`.
    pub line_width: f32,
    /// `J` and `j`, or an ExtGState's `/LC` and `/LJ`: a number that's no
    /// cap is a butt cap, and one that's no join a miter join, as PDFium
    /// draws them.
    pub line_cap: LineCap,
    pub line_join: LineJoin,
    /// `M`, or an ExtGState's `/ML`.
    pub miter_limit: f32,
    /// What drawing is cut to: the clips content made, and the `/BBox` of
    /// each form it's drawn in.
    pub clip: Clip,
    pub text: TextState,
}

/// The text state, and where text goes next.
#[derive(Debug, Clone)]
pub struct TextState {
    /// The font `Tf`, or an ExtGState's `/Font`, picked, among the
    /// interpretation's ([`FontScope`](crate::content::FontScope)); a
    /// string shown has it ([`TextShow::font`](super::TextShow::font)).
    pub(crate) font: Option<FontId>,
    /// Whether that's PDFium's stock Helvetica, the resources having none
    /// by the name it gave.
    pub stock: bool,
    /// `Tf`'s size: 1 before one.
    pub size: f32,
    /// `Tc`.
    pub char_space: f32,
    /// `Tw`: added after each code 32 a single byte is.
    pub word_space: f32,
    /// `Tz` as a fraction: 1 is 100.
    pub scale: f32,
    /// `TL`.
    pub leading: f32,
    /// `Ts`.
    pub rise: f32,
    /// `Tr`.
    pub mode: TextMode,
    /// The text matrix: what `Tm` set, or the identity `BT` set.
    pub matrix: Affine,
    /// Where the line starts, in the text matrix's space: `Td`, `TD` and
    /// `T*` move it, and `Tm` and `BT` put it at 0.
    pub line: Pt,
    /// Where the next string starts: where the line does, moved on by the
    /// strings shown since.
    pub pen: Pt,
    /// Whether where the pen is is guessed: a string shown since the line
    /// was set is in a font whose codes or widths are guessed, or shows a
    /// code whose width is ([`FontApprox`], [`Foreign::width_guessed`]).
    ///
    /// [`FontApprox`]: crate::content::FontApprox
    /// [`Foreign::width_guessed`]: crate::content::Foreign::width_guessed
    pub guessed: bool,
}

impl Default for State {
    fn default() -> State {
        State {
            ctm: Affine::IDENTITY,
            line_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            clip: Clip::default(),
            text: TextState {
                font: None,
                stock: false,
                size: 1.0,
                char_space: 0.0,
                word_space: 0.0,
                scale: 1.0,
                leading: 0.0,
                rise: 0.0,
                mode: TextMode::Fill,
                matrix: Affine::IDENTITY,
                line: ORIGIN,
                pen: ORIGIN,
                guessed: false,
            },
        }
    }
}

const ORIGIN: Pt = Pt { x: 0.0, y: 0.0 };

impl State {
    /// A box that holds what the clip leaves ([`Clip::bounds`]): None
    /// where nothing cuts what's drawn.
    pub fn clip_bounds(&self) -> Option<Rect> {
        self.clip.bounds()
    }

    /// Whether where it draws is finite: the CTM, where the line starts
    /// and the pen. Operands are, but what they're added or multiplied to
    /// may not be.
    pub(super) fn is_finite(&self) -> bool {
        let text = &self.text;
        finite(self.ctm)
            && [text.line, text.pen]
                .iter()
                .all(|p| p.x.is_finite() && p.y.is_finite())
    }

    /// The state a form drawn in this one starts in, its `/Matrix`
    /// `matrix` onto PageSpace, as PDFium starts it (`AddForm`): the
    /// font, its size, `Tc`, `Tw`, `Tr` and the line's width, caps, joins
    /// and miter limit carried in, and the rest of the text state as
    /// before any operator, so `Tz`, `TL` and `Ts` aren't. What it draws
    /// is cut to the clip it's drawn in, and to its `bbox` through
    /// `matrix`, where it has one.
    pub(super) fn form(&self, matrix: Affine, bbox: Option<Rect>) -> State {
        let text = &self.text;
        let fresh = State::default();
        let clip = match bbox {
            // As PDFium's `CFX_Path::AppendRect` draws it.
            Some(r) => {
                let corners = [(r.x0, r.y0), (r.x0, r.y1), (r.x1, r.y1), (r.x1, r.y0)]
                    .map(|(x, y)| matrix.apply(Pt { x, y }));
                let mut segs = vec![Seg::Move(corners[0])];
                segs.extend(
                    corners[1..]
                        .iter()
                        .chain(&corners[..1])
                        .map(|&p| Seg::Line(p)),
                );
                segs.push(Seg::Close);
                let bounds = super::path::bounds(&segs);
                self.clip.with_path(&segs, FillRule::NonZero, bounds)
            }
            None => self.clip.clone(),
        };
        State {
            ctm: matrix,
            line_width: self.line_width,
            line_cap: self.line_cap,
            line_join: self.line_join,
            miter_limit: self.miter_limit,
            clip,
            text: TextState {
                font: text.font,
                stock: text.stock,
                size: text.size,
                char_space: text.char_space,
                word_space: text.word_space,
                mode: text.mode,
                ..fresh.text
            },
        }
    }

    /// What `op`, an operator that only sets the state, does to it, its
    /// operands `step`'s; any other does nothing.
    pub(super) fn set(&mut self, op: Op, step: &Step<'_, '_>) {
        let text = &mut self.text;
        match op {
            Op::Transform => {
                if let Some(m) = matrix(step) {
                    self.ctm = m.then(self.ctm);
                }
            }
            Op::LineWidth => set(&mut self.line_width, step),
            Op::LineCap => self.line_cap = cap(integer(step)),
            Op::LineJoin => self.line_join = join(integer(step)),
            Op::MiterLimit => set(&mut self.miter_limit, step),
            Op::BeginText => {
                text.matrix = Affine::IDENTITY;
                text.line = ORIGIN;
                text.pen = ORIGIN;
                text.guessed = false;
            }
            Op::TextMatrix => {
                if let Some(m) = matrix(step) {
                    text.matrix = m;
                    text.line = ORIGIN;
                    text.pen = ORIGIN;
                    text.guessed = false;
                }
            }
            Op::MoveLine | Op::MoveLineLeading => {
                if let Some([x, y]) = nums(step) {
                    text.line.x += x;
                    text.line.y += y;
                    text.pen = text.line;
                    text.guessed = false;
                    if op == Op::MoveLineLeading {
                        text.leading = -y;
                    }
                }
            }
            Op::NextLine => text.next_line(),
            Op::CharSpace => set(&mut text.char_space, step),
            Op::WordSpace => set(&mut text.word_space, step),
            // PDFium takes `Tz` with one operand only.
            Op::Scale if step.operands.len() == 1 => {
                if let Some([percent]) = nums(step) {
                    text.scale = percent / 100.0;
                }
            }
            Op::Scale => {}
            Op::Leading => set(&mut text.leading, step),
            Op::Rise => set(&mut text.rise, step),
            Op::RenderMode => {
                // A number past an `int` is 0, as PDFium reads it, and one
                // that's no mode sets none.
                if let Some(mode) = nums(step).and_then(|[n]| TextMode::of(int(n))) {
                    text.mode = mode;
                }
            }
            _ => {}
        }
    }

    /// What `'` and `"` do before the string they show, their operands
    /// `step`'s: `"` sets `Tw` and `Tc` to its first two, and both move to
    /// the next line.
    pub(super) fn next_line_of(&mut self, op: Op, step: &Step<'_, '_>) {
        if op == Op::NextLineShowSpaced
            && let Some([tw, tc, _]) = nums(step)
        {
            self.text.word_space = tw;
            self.text.char_space = tc;
        }
        self.text.next_line();
    }

    /// The state `op`, an operator that moves text to a line, leaves, its
    /// operands `step`'s: `Td`, `TD` and `T*`'s, and `'` and `"`'s before
    /// the string they show.
    pub(super) fn moved(&self, op: &[u8], step: &Step<'_, '_>) -> State {
        let mut moved = self.clone();
        match Op::of(op) {
            op @ (Op::NextLineShow | Op::NextLineShowSpaced) => moved.next_line_of(op, step),
            op => moved.set(op, step),
        }
        moved
    }
}

impl TextState {
    /// `T*`, and what `'` and `"` do first: the line moved down by the
    /// leading.
    pub(super) fn next_line(&mut self) {
        self.line.y -= self.leading;
        self.pen = self.line;
        self.guessed = false;
    }
}

/// Whether `m`'s numbers are all finite.
pub(super) fn finite(m: Affine) -> bool {
    [m.a, m.b, m.c, m.d, m.e, m.f].iter().all(|v| v.is_finite())
}

/// A number as an `int`, as PDFium casts one: 0 where it's past one.
pub(super) fn int(n: f32) -> i32 {
    if (-2_147_483_648.0..2_147_483_648.0).contains(&n) {
        n as i32
    } else {
        0
    }
}

/// The last operand as an `int`, 0 where it isn't finite.
fn integer(step: &Step<'_, '_>) -> i32 {
    nums(step).map_or(0, |[n]| int(n))
}

/// The cap `n` is (`J`, `/LC`).
pub(super) fn cap(n: i32) -> LineCap {
    match n {
        1 => LineCap::Round,
        2 => LineCap::Square,
        _ => LineCap::Butt,
    }
}

/// The join `n` is (`j`, `/LJ`).
pub(super) fn join(n: i32) -> LineJoin {
    match n {
        1 => LineJoin::Round,
        2 => LineJoin::Bevel,
        _ => LineJoin::Miter,
    }
}

/// `slot` set to the last operand, where it's finite.
fn set(slot: &mut f32, step: &Step<'_, '_>) {
    if let Some([n]) = nums(step) {
        *slot = n;
    }
}

/// The last six operands as a matrix.
fn matrix(step: &Step<'_, '_>) -> Option<Affine> {
    let [a, b, c, d, e, f] = nums(step)?;
    Some(Affine { a, b, c, d, e, f })
}

/// The last `N` operands as numbers, in order ([`number`]): None where
/// one isn't finite.
pub(super) fn nums<const N: usize>(step: &Step<'_, '_>) -> Option<[f32; N]> {
    let n: [f32; N] = std::array::from_fn(|k| step.arg(N - 1 - k).map_or(0.0, number));
    n.iter().all(|v| v.is_finite()).then_some(n)
}

/// A number as PDFium reads one (`FX_Number`), 0 for an operand that
/// isn't one:
/// - with a point, the longest decimal it starts with, past the spaces
///   and signs before it but a minus right before its digits;
/// - without, a whole number, its one sign and the digits after it, 0
///   where it's past 32 bits, or past an `int` with a sign.
///
/// One with a point too large for an `f32` is infinite.
pub(crate) fn number(operand: &[u8]) -> f32 {
    let digits = |s: &[u8]| s.iter().take_while(|c| c.is_ascii_digit()).count();
    if !operand
        .first()
        .is_some_and(|c| matches!(c, b'0'..=b'9' | b'+' | b'-' | b'.'))
    {
        return 0.0;
    }
    if operand.contains(&b'.') {
        let signs = operand
            .iter()
            .take_while(|c| matches!(c, b' ' | b'+' | b'-'))
            .count();
        let from = signs - usize::from(signs > 0 && operand[signs - 1] == b'-');
        let s = &operand[from..];
        let mut end = usize::from(s.first() == Some(&b'-'));
        let whole = digits(&s[end..]);
        end += whole;
        let fraction = match s.get(end) {
            Some(b'.') => digits(&s[end + 1..]),
            _ => 0,
        };
        if whole + fraction == 0 {
            return 0.0;
        }
        if s.get(end) == Some(&b'.') {
            end += 1 + fraction;
        }
        return std::str::from_utf8(&s[..end])
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0.0);
    }
    let (sign, rest) = match operand[0] {
        b'-' => (Some(true), &operand[1..]),
        b'+' => (Some(false), &operand[1..]),
        _ => (None, operand),
    };
    let mut value: u64 = 0;
    for &c in &rest[..digits(rest)] {
        value = value * 10 + u64::from(c - b'0');
        if value > u64::from(u32::MAX) {
            return 0.0;
        }
    }
    match sign {
        None => value as f32,
        Some(negative) if value > u64::from(i32::MAX.unsigned_abs()) + u64::from(negative) => 0.0,
        Some(true) => -(value as f32),
        Some(false) => value as f32,
    }
}
