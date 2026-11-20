//! Minimal content-stream operation builder for test fixtures and examples,
//! plus test-only `Content::decode` backed by `pdf-content-streams` (M18
//! slices 11 and 13). Production content-stream lexing, splicing, and writing
//! live in `crate::content`.

use pdf_objects::Object;

use super::{Error, Result};

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
        let limits = pdf_content_streams::Limits::DEFAULT;
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
