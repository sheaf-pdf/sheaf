//! PDF text strings ([`text_string`]), and, for tests, a whole document
//! written as a save writes it: [`rewrite`] and [`rewrite_to_path`], an
//! export of it with no transform ([`Export::whole`]), its encryption and
//! object numbers kept.
//!
//! Every way out of a document is an export ([`crate::export`]) or a
//! save: [`materialize::save_with`]'s (the command line's
//! `Session::save`), or the app's through [`materialize::write_file`].
//!
//! [`materialize::save_with`]: crate::materialize::save_with
//! [`materialize::write_file`]: crate::materialize::write_file

use lopdf::{Object, StringFormat};

#[cfg(any(test, feature = "fixtures"))]
pub use whole::{rewrite, rewrite_to_path};

/// A whole document written as a save writes it, for tests.
#[cfg(any(test, feature = "fixtures"))]
mod whole {
    use std::path::Path;

    use anyhow::Result;
    use in_progress::NoProgress;

    use crate::export::{self, Export, Out};
    use crate::materialize::AskAnswers;
    use crate::model::Document;
    use crate::tx::Actor;

    /// What a whole document's Rewrite answers what its plan asks, as
    /// print does: it's written as asked, so the signatures it can't keep
    /// are broken, and an XFA part that holds values goes when save policy
    /// drops it.
    const ANSWERS: AskAnswers = export::PRINT_ANSWERS;

    /// `doc` as a complete new PDF: what a save writes, the primary's
    /// encryption and object numbers kept.
    pub fn rewrite(doc: &Document) -> Result<Vec<u8>> {
        let out = exported_whole(doc, Out::Bytes)?;
        Ok(out.expect("a Rewrite to bytes is its bytes"))
    }

    /// `doc` rewritten to `path` durably (a temp file renamed over it), as
    /// a save would ([`rewrite`]): never the document's own file.
    pub fn rewrite_to_path(doc: &Document, path: &Path) -> Result<()> {
        let out = Out::File {
            path: path.into(),
            overwrite: true,
        };
        exported_whole(doc, out)?;
        Ok(())
    }

    /// `doc` exported whole to `out` ([`Export::whole`]): what it says of
    /// the file is logged.
    fn exported_whole(doc: &Document, out: Out) -> Result<Option<Vec<u8>>> {
        let e = Export::whole(doc.export_from(Actor::User), out);
        let exported = export::run(e, &ANSWERS, &NoProgress)?;
        for n in &exported.notes {
            log::warn!("{n}");
        }
        Ok(exported.bytes)
    }
}

/// PDF text string: as is when PDFDocEncoding reads it back so
/// (`file::is_pdf_doc_ascii`), otherwise UTF-16BE with BOM.
pub fn text_string(s: &str) -> Object {
    if crate::file::is_pdf_doc_ascii(s) {
        Object::String(s.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        Object::String(bytes, StringFormat::Hexadecimal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::FixedCount;
    use crate::materialize::rewrite_tests::{make_pdf, open, read_pages, tempdir};

    #[test]
    fn rewrite_to_path_is_atomic() {
        let dir = tempdir();
        let a = make_pdf(&dir, "a.pdf", "A", 2, &[]);
        let model = open(&a, &[], Box::new(FixedCount(10)));
        let out = dir.join("out.pdf");
        rewrite_to_path(&model, &out).unwrap();
        assert!(out.exists());
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(
            !names
                .iter()
                .any(|n| n.to_string_lossy().ends_with(".sheaf-tmp")),
            "{names:?}"
        );
        assert_eq!(read_pages(&std::fs::read(&out).unwrap()).len(), 2);
        // Never over the document's own file.
        assert!(rewrite_to_path(&model, &a).is_err());
    }
}
