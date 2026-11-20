//! Content through `pdf-content-streams` (M18 slices 11, 13 and 19): the
//! types content is written with ([`ContentWriter`], and what its calls
//! take), the resources it draws with by what they are ([`Res`], named
//! when the content is finished against the names a resources dictionary
//! already binds, [`Reserved`]), a page's streams as one ([`Concat`],
//! joined by [`Join`], with the places in them, [`Pos`] and
//! [`SpanAcross`]), and what its reading is held to and needs of the
//! resources ([`Limits`], [`ResCx`]), and its splice ([`splice`],
//! [`splice_each`], [`without_keys`]), which sheaf splices through, with
//! content's tokens and an operand as it reads them ([`tokens`],
//! [`object`]), as its splice reads content and a list.
//! `crate::content` re-exports them, and is where sheaf lexes content.
//!
//! Also a minimal content-stream operation builder for test fixtures and
//! examples ([`Content`]), with a test-only `Content::decode`.

use pdf_objects::Object;

use super::{Error, Result};

pub use pdf_content_streams::{
    Blend, Color, Concat, ContentWriter, FillRule, GState, Join, Limits, LineCap, LineJoin, Need,
    Paint, Pos, Res, ResBody, ResCx, ResId, ResKind, Reserved, ResourceNeeds, Rewrite, SpanAcross,
    SpliceError, Spliced, TextMode, object, splice, splice_each, tokens, without_keys,
};

#[cfg(test)]
pub(crate) mod read_back;

/// A single PDF content-stream operator and its operands in stream order.
#[derive(Debug, Clone, PartialEq)]
pub struct Operation {
    pub operator: String,
    pub operands: Vec<Object>,
}

impl Operation {
    #[must_use]
    pub fn new(operator: impl Into<String>, operands: Vec<Object>) -> Self {
        Self {
            operator: operator.into(),
            operands,
        }
    }
}

/// Sequence of content-stream [`Operation`]s.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Content {
    pub operations: Vec<Operation>,
}

impl Content {
    /// Serializes `self.operations` into PDF content-stream syntax.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        for (idx, op) in self.operations.iter().enumerate() {
            if idx > 0 {
                out.push(b'\n');
            }
            for operand in &op.operands {
                pdf_objects::serialize::write(&mut out, operand)
                    .map_err(|e| Error::Syntax(e.to_string()))?;
                out.push(b' ');
            }
            out.extend_from_slice(op.operator.as_bytes());
        }
        Ok(out)
    }

    /// Parses `bytes` into [`Content`] through `pdf-content-streams` (test
    /// oracle for `content::corpus`, `forms::check`, and `file::tests`).
    #[cfg(test)]
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let limits = Limits::DEFAULT;
        let res = pdf_content_streams::ResCx::none();
        let mut operations = Vec::new();
        for item in pdf_content_streams::operations(bytes, &res, &limits) {
            let Ok(op) = item else { break };
            let operator = String::from_utf8_lossy(op.name()).into_owned();
            let mut operands = Vec::with_capacity(op.operands().len());
            for operand in op.operands() {
                if let Some(obj) = operand.object(&limits) {
                    operands.push(obj);
                }
            }
            operations.push(Operation { operator, operands });
        }
        Ok(Self { operations })
    }
}
