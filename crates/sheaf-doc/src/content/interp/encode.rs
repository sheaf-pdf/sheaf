//! Content written again from what it draws (M11 slice 3): each operator
//! of a page's content, its streams as one, and of each form it draws, as
//! written, on [`ContentWriter`], but those that move text to a line,
//! which are written from the state the interpreter reads: `Td`, `TD` and
//! `T*` as a `Tm` of the text matrix and a `Td` from its origin to the
//! line they move to, `'` as those and a `Tj`, and `"` as `Tw`, `Tc`,
//! those and a `Tj`. No line then depends on the leading, or on where a
//! line before it was, so a line can be taken out, or moved, without
//! moving the lines after it; and PDFium places the line by the sums it
//! placed it by before, so the text after it is where it was to the bit,
//! as one `Tm` of the line itself, its position folded into the matrix,
//! wouldn't leave it. Strings on one line still follow each other: a `Tj`
//! or `TJ` starts where the one before it on its line ended, so where
//! glyphs are taken out of one, the next is placed again too (a `Tm` and
//! an absolute `Td` to where it started: M11 slice 5).
//!
//! A number so worked out is written as the `f32` the state holds
//! ([`object::real`]), which reads back as itself but past its bounds;
//! every other operand is as written, and the resources keep the names
//! the content gave them, so it needs nothing new of them. (A `Tf` of a
//! font sheaf writes, as redaction will, is named by its resource then:
//! M11 slice 5.)
//!
//! Its production caller is M11 slice 5, which writes what redaction
//! leaves of a page this way; until then its users are the tests, the
//! `interp` fuzz target and the PDFium corpus tier.

use std::collections::HashSet;
use std::ops::ControlFlow;

use lopdf::{Object, ObjectId};

use super::{Event, Forms, InterpError, Kind, Ops, Opts, Wants, interpret};
use crate::annot::Pt;
use crate::content::lex;
use crate::content::page::{Concat, ReadObjects};
use crate::content::{ContentWriter, Fonts, Reserved, Tok};
use crate::geom::Affine;
use crate::serialize::object;
use crate::source::Limits;

/// Content written again ([`encode`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Encoded {
    /// The content's own operators, its streams as one.
    pub content: Vec<u8>,
    /// Each form it draws, once a form, in the order they're first drawn:
    /// its content written the same way. A form only ever refused isn't
    /// among them, its content being drawn nowhere.
    pub forms: Vec<(ObjectId, Vec<u8>)>,
}

/// Content that couldn't be written again.
#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    #[error(transparent)]
    Interp(#[from] InterpError),
    /// What the operator at `at` in form `form`'s content, or in the
    /// content encoded, does can't be written so it reads back the same.
    #[error("{why}, at byte {at}{}", .form.map_or(String::new(), |f| format!(" in form {} {} R", f.0, f.1)))]
    Unwritable {
        form: Option<ObjectId>,
        at: usize,
        why: Unwritable,
    },
}

/// Why an operator can't be written again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Unwritable {
    /// It moves text to a line past what an `f32` holds: no number says
    /// where it is.
    #[error("text moved past what a number holds")]
    Line,
    /// An inline image whose `/L` isn't its data's length, which its
    /// filter or its `EI` said instead ([`lex`](crate::content::lex)):
    /// with more content after it, the `/L` could reach, and the image
    /// end elsewhere.
    #[error("an inline image whose /L isn't its data's length")]
    InlineLength,
}

/// `content`, drawn with `resources` (a page's `/Resources` as written)
/// and its fonts read through `fonts`, written again: interpreted, its
/// forms drawn ([`Forms::Descend`]), each operator written where it's
/// interpreted, a form's where its first use is.
pub fn encode<R: ReadObjects>(
    objects: &R,
    content: &Concat,
    resources: Option<&Object>,
    lim: &Limits,
    fonts: &Fonts,
) -> Result<Encoded, EncodeError> {
    let mut page = ContentWriter::new();
    let mut forms: Vec<(ObjectId, ContentWriter)> = Vec::new();
    let mut written = HashSet::new();
    // Each form use being drawn: its form, and which of `forms` it
    // writes, where it's the form's first.
    let mut uses: Vec<(ObjectId, Option<usize>)> = Vec::new();
    let mut unwritable = None;
    let opts = Opts {
        forms: Forms::Descend,
        wants: Wants {
            ops: Ops::ALL,
            ..Wants::NONE
        },
    };
    interpret(objects, content, resources, lim, fonts, opts, |event| {
        match event.kind {
            Kind::FormBegin(form) => {
                let first = written.insert(form.form).then(|| {
                    forms.push((form.form, ContentWriter::new()));
                    forms.len() - 1
                });
                uses.push((form.form, first));
            }
            Kind::FormEnd(_) => {
                uses.pop();
            }
            Kind::Op => {
                let (form, out) = match uses.last() {
                    None => (None, &mut page),
                    Some(&(form, Some(i))) => (Some(form), &mut forms[i].1),
                    Some((_, None)) => return ControlFlow::Continue(()),
                };
                if let Err(why) = write(out, &event, lim) {
                    let at = event.at.op.start;
                    unwritable = Some(EncodeError::Unwritable { form, at, why });
                    return ControlFlow::Break(());
                }
            }
            _ => {}
        }
        ControlFlow::Continue(())
    })?;
    if let Some(e) = unwritable {
        return Err(e);
    }
    let bytes = |w: ContentWriter| w.finish(&Reserved::default()).0;
    Ok(Encoded {
        content: bytes(page),
        forms: forms.into_iter().map(|(id, w)| (id, bytes(w))).collect(),
    })
}

/// `event`'s operator written to `out`: as written, but one that moves
/// text to a line, written from the line the interpreter moves it to: the
/// text matrix, and a move to the line from its origin where the line
/// isn't there.
fn write(out: &mut ContentWriter, event: &Event<'_>, lim: &Limits) -> Result<(), Unwritable> {
    let at = &event.at;
    let op = match at.tok {
        Tok::Op(op @ (b"Td" | b"TD" | b"T*" | b"'" | b"\"")) => op,
        Tok::Op(op) => {
            out.operation(at.operands(), op);
            return Ok(());
        }
        Tok::InlineImage(ref data) => {
            let image = at.op.clone();
            let given = lex::image_length(at.content(), image.clone(), lim.content.depth);
            if given.is_some_and(|l| l != data.len() as u64) {
                return Err(Unwritable::InlineLength);
            }
            out.operation(at.operands(), &at.content()[image]);
            return Ok(());
        }
        Tok::Operand(_) => return Ok(()),
    };
    let before = &event.state.text;
    let moved = event.state.moved(op, &at.step());
    let after = &moved.text;
    // A `Td` that moves neither the line nor the pen does nothing but end
    // PDFium's own reading of the path operators after an `m`, as any
    // operator does: it's written as it is.
    if matches!(op, b"Td" | b"TD") && after.line == before.line && after.pen == before.pen {
        out.operation(at.operands(), op);
        return Ok(());
    }
    if op == b"\"" {
        out.operation([&*real(after.word_space)?], b"Tw");
        out.operation([&*real(after.char_space)?], b"Tc");
    }
    let Affine { a, b, c, d, e, f } = after.matrix;
    let tm = [a, b, c, d, e, f].map(real);
    let tm: Vec<Vec<u8>> = tm.into_iter().collect::<Result<_, _>>()?;
    out.operation(tm.iter().map(Vec::as_slice), b"Tm");
    if after.line != (Pt { x: 0.0, y: 0.0 }) {
        let td = [real(after.line.x)?, real(after.line.y)?];
        out.operation(td.iter().map(Vec::as_slice), b"Td");
    }
    if matches!(op, b"'" | b"\"")
        && let Some(string) = at.operand(0)
    {
        out.operation([string], b"Tj");
    }
    Ok(())
}

/// `x` as an operand that reads back as itself.
fn real(x: f32) -> Result<Vec<u8>, Unwritable> {
    let mut out = Vec::new();
    object::real(&mut out, f64::from(x)).map_err(|_| Unwritable::Line)?;
    Ok(out)
}
