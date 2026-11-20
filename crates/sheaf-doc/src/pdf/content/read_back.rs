//! Content read back through pdf-content-streams' tokens, for the tests
//! that hold what sheaf writes (`content::written_golden`): each
//! operation's operands and operator, as written.

use pdf_content_streams::{Limits, ResCx, Tok, tokens};
use pdf_objects::{Object, resolve_fn};

use crate::pdf::Document;

/// An operation as read: its operands, each as written, and its operator
/// (an inline image's is its token whole, `BI` to `EI`).
pub(crate) type Operation<'a> = (Vec<&'a [u8]>, &'a [u8]);

/// The resources content is drawn with, `/Resources` as written, and the
/// file they're in: None for content that names no color space.
pub(crate) type Drawn<'a> = Option<(&'a Document, Option<&'a Object>)>;

/// `content`'s operations, read with the color spaces `with` names, and
/// how many operands at its end no operator takes; or the first token
/// that doesn't read, as text.
pub(crate) fn operations<'a>(
    content: &'a [u8],
    with: Drawn<'_>,
) -> Result<(Vec<Operation<'a>>, usize), String> {
    let limits = Limits::DEFAULT;
    let res = match with {
        None => ResCx::none(),
        Some((pdf, resources)) => {
            let objects = resolve_fn(|id| pdf.objects.get(&id));
            ResCx::read(objects, resources, &limits).map_err(|e| e.to_string())?
        }
    };
    let mut operations = Vec::new();
    let mut operands = Vec::new();
    for token in tokens(content, &res, &limits) {
        let (at, tok) = token.map_err(|e| e.to_string())?;
        match tok {
            Tok::Operand(operand) => operands.push(operand),
            Tok::Op(op) => operations.push((std::mem::take(&mut operands), op)),
            Tok::InlineImage(_) => {
                operations.push((std::mem::take(&mut operands), &content[at]));
            }
        }
    }
    Ok((operations, operands.len()))
}
