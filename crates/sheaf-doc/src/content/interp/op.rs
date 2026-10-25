//! The operators the interpreter reads, each told by its name once a step,
//! for the path, the state and the rest to read: one it doesn't read is
//! [`Op::Other`], as PDFium passes over an operator it doesn't know. A
//! visitor names those it wants to be handed as a set of them ([`Ops`]).

use crate::content::{FillRule, Paint};

/// An operator, by what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Op {
    /// `m`.
    Move,
    /// `l`.
    Line,
    /// `c`.
    Curve,
    /// `v`: a curve whose first control point is where the pen is.
    CurveFromPen,
    /// `y`: a curve whose second control point is where it ends.
    CurveToEnd,
    /// `h`.
    ClosePath,
    /// `re`.
    Rect,
    /// `W` and `W*`: the path made the clip, by the rule.
    Clip(FillRule),
    /// A painting operator, `F` read as `f`.
    Paint(Paint),
    /// `cm`.
    Transform,
    /// `w`.
    LineWidth,
    /// `J`.
    LineCap,
    /// `j`.
    LineJoin,
    /// `M`.
    MiterLimit,
    /// `q`.
    Save,
    /// `Q`.
    Restore,
    /// `gs`.
    ExtGState,
    /// `BT`.
    BeginText,
    /// `ET`.
    EndText,
    /// `Tm`.
    TextMatrix,
    /// `Td`.
    MoveLine,
    /// `TD`: `Td`, the leading set too.
    MoveLineLeading,
    /// `T*`.
    NextLine,
    /// `Tc`.
    CharSpace,
    /// `Tw`.
    WordSpace,
    /// `Tz`.
    Scale,
    /// `TL`.
    Leading,
    /// `Ts`.
    Rise,
    /// `Tr`.
    RenderMode,
    /// `Tf`.
    Font,
    /// `Tj`.
    Show,
    /// `TJ`.
    ShowArray,
    /// `'`.
    NextLineShow,
    /// `"`: `'`, `Tw` and `Tc` set first.
    NextLineShowSpaced,
    /// `Do`.
    Draw,
    /// `sh`.
    Shade,
    /// `BMC`.
    BeginMarked,
    /// `BDC`.
    BeginMarkedProps,
    /// `EMC`.
    EndMarked,
    /// `MP`.
    Point,
    /// `DP`.
    PointProps,
    /// Any other, and what isn't an operator: an inline image.
    Other,
}

impl Op {
    /// The operator named `name`.
    pub(super) const fn of(name: &[u8]) -> Op {
        match name {
            b"m" => Op::Move,
            b"l" => Op::Line,
            b"c" => Op::Curve,
            b"v" => Op::CurveFromPen,
            b"y" => Op::CurveToEnd,
            b"h" => Op::ClosePath,
            b"re" => Op::Rect,
            b"W" => Op::Clip(FillRule::NonZero),
            b"W*" => Op::Clip(FillRule::EvenOdd),
            b"f" | b"F" => Op::Paint(Paint::Fill),
            b"f*" => Op::Paint(Paint::FillEvenOdd),
            b"S" => Op::Paint(Paint::Stroke),
            b"s" => Op::Paint(Paint::CloseStroke),
            b"B" => Op::Paint(Paint::FillStroke),
            b"B*" => Op::Paint(Paint::FillStrokeEvenOdd),
            b"b" => Op::Paint(Paint::CloseFillStroke),
            b"b*" => Op::Paint(Paint::CloseFillStrokeEvenOdd),
            b"n" => Op::Paint(Paint::None),
            b"cm" => Op::Transform,
            b"w" => Op::LineWidth,
            b"J" => Op::LineCap,
            b"j" => Op::LineJoin,
            b"M" => Op::MiterLimit,
            b"q" => Op::Save,
            b"Q" => Op::Restore,
            b"gs" => Op::ExtGState,
            b"BT" => Op::BeginText,
            b"ET" => Op::EndText,
            b"Tm" => Op::TextMatrix,
            b"Td" => Op::MoveLine,
            b"TD" => Op::MoveLineLeading,
            b"T*" => Op::NextLine,
            b"Tc" => Op::CharSpace,
            b"Tw" => Op::WordSpace,
            b"Tz" => Op::Scale,
            b"TL" => Op::Leading,
            b"Ts" => Op::Rise,
            b"Tr" => Op::RenderMode,
            b"Tf" => Op::Font,
            b"Tj" => Op::Show,
            b"TJ" => Op::ShowArray,
            b"'" => Op::NextLineShow,
            b"\"" => Op::NextLineShowSpaced,
            b"Do" => Op::Draw,
            b"sh" => Op::Shade,
            b"BMC" => Op::BeginMarked,
            b"BDC" => Op::BeginMarkedProps,
            b"EMC" => Op::EndMarked,
            b"MP" => Op::Point,
            b"DP" => Op::PointProps,
            _ => Op::Other,
        }
    }

    /// Whether it builds the path ([`Builder::build`]).
    ///
    /// [`Builder::build`]: super::path::Builder::build
    pub(super) fn builds_path(self) -> bool {
        matches!(
            self,
            Op::Move
                | Op::Line
                | Op::Curve
                | Op::CurveFromPen
                | Op::CurveToEnd
                | Op::ClosePath
                | Op::Rect
                | Op::Clip(_)
        )
    }

    /// Its place in a set of operators ([`Ops`]): each its own, and every
    /// one the interpreter doesn't read one.
    const fn bit(self) -> u64 {
        let place = match self {
            Op::Move => 0,
            Op::Line => 1,
            Op::Curve => 2,
            Op::CurveFromPen => 3,
            Op::CurveToEnd => 4,
            Op::ClosePath => 5,
            Op::Rect => 6,
            Op::Clip(FillRule::NonZero) => 7,
            Op::Clip(FillRule::EvenOdd) => 8,
            // 9 to 17.
            Op::Paint(paint) => 9 + paint as u32,
            Op::Transform => 18,
            Op::LineWidth => 19,
            Op::LineCap => 20,
            Op::LineJoin => 21,
            Op::MiterLimit => 22,
            Op::Save => 23,
            Op::Restore => 24,
            Op::ExtGState => 25,
            Op::BeginText => 26,
            Op::EndText => 27,
            Op::TextMatrix => 28,
            Op::MoveLine => 29,
            Op::MoveLineLeading => 30,
            Op::NextLine => 31,
            Op::CharSpace => 32,
            Op::WordSpace => 33,
            Op::Scale => 34,
            Op::Leading => 35,
            Op::Rise => 36,
            Op::RenderMode => 37,
            Op::Font => 38,
            Op::Show => 39,
            Op::ShowArray => 40,
            Op::NextLineShow => 41,
            Op::NextLineShowSpaced => 42,
            Op::Draw => 43,
            Op::Shade => 44,
            Op::BeginMarked => 45,
            Op::BeginMarkedProps => 46,
            Op::EndMarked => 47,
            Op::Point => 48,
            Op::PointProps => 49,
            Op::Other => 50,
        };
        1 << place
    }
}

/// Operators, as a set: those a visitor wants to be handed as a
/// [`Kind::Op`](super::Kind::Op) ([`Wants::ops`](super::Wants::ops)).
/// Those the interpreter doesn't read are one, with inline images, and
/// `F` is `f`, as PDFium reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ops(u64);

impl Ops {
    pub const ALL: Ops = Ops(u64::MAX);
    pub const NONE: Ops = Ops(0);

    /// The operators named `names`.
    pub const fn of(names: &[&[u8]]) -> Ops {
        let mut bits = 0;
        let mut i = 0;
        while i < names.len() {
            bits |= Op::of(names[i]).bit();
            i += 1;
        }
        Ops(bits)
    }

    /// Whether it holds `op`.
    pub(super) fn has(self, op: Op) -> bool {
        self.0 & op.bit() != 0
    }
}
