//! The single door to the PDF layer (M18 slices 1 and 8–13): every module in
//! `sheaf-doc` and every other crate in the workspace reaches the PDF
//! object, structure, filter and encryption types through here
//! (`pdf-objects`, `pdf-structure`, `pdf-filters`, `pdf-crypt`), and
//! content's (`content`: `pdf-content-streams`, M18 slice 19), fonts'
//! (`fonts`: `pdf-fonts` and `pdf-gfx`, M18 slice 22), and content
//! interpreted and written again (`gfx`: `pdf-gfx`, M18 slice 23),
//! annotations' (`annots`: `pdf-annots`, M18 slice 24), text fields'
//! appearances (`forms`: `pdf-forms`, M10 slice 6), and text strings
//! (`text`: `pdf-objects`) and the common structures (`semantics`: dates,
//! name and number trees, the page tree's walk and XMP's claims,
//! `pdf-semantics`, M18 slice 21), with where its readers tell what they
//! read past (`fault`).

pub mod annots;
pub mod content;
mod document;
mod error;
pub mod fonts;
pub mod forms;
pub mod gfx;
mod reader;
pub mod semantics;
mod stream;
#[cfg(any(test, feature = "fixtures"))]
pub mod test_crypt;
pub mod text;
pub mod write;

pub use document::Document;
pub use error::{DecryptionError, Error, Limit, Result};
#[cfg(any(test, feature = "fixtures"))]
pub use pdf_crypt::Ivs;
pub use pdf_crypt::{Changes, KeyCheck, Method, Password, Permissions, Printing, Protection};
pub use pdf_objects::{
    Data, Dict as Dictionary, Entry as DictEntry, Form as StringFormat, Name, Object, ObjectId,
    Resolve, SmallBytes, Span, Str, Stream, Streams, Unreadable, Value, Why, resolve_fn,
};
pub use pdf_structure::{ByteSource, FileBytes};
pub use reader::{Handover, LazyPdf, RawCopies, ReadLimits, XrefFacts, XrefKind};
pub use stream::{StreamExt, deflate, deflate_best, deflate_fast, deflate_options, inflate};

/// Where a family reader tells what it read past and went on: a [`Sink`]
/// of faults, each at its [`Place`]. `()` is the sink that wants none.
///
/// [`Sink`]: fault::Sink
/// [`Place`]: fault::Place
pub mod fault {
    pub use pdf_objects::fault::{Place, Sink};
}

/// Constructs a literal PDF byte string (`(…)`), setting `Some(StringFormat::Literal)`
/// so `Object::eq` matches strings parsed from `(…)` tokens.
#[inline]
#[must_use]
pub fn literal(bytes: impl Into<SmallBytes>) -> Object {
    Object::new(Value::String(
        Str::bytes(bytes),
        Some(StringFormat::Literal),
    ))
}

#[doc(hidden)]
#[macro_export]
macro_rules! __pdf_dictionary {
    () => {
        $crate::pdf::Dictionary::new()
    };
    ($( $key:expr => $value:expr ),+ ,) => {
        $crate::__pdf_dictionary!( $($key => $value),+ )
    };
    ($( $key:expr => $value:expr ),*) => {{
        let mut dict = $crate::pdf::Dictionary::new();
        $(
            dict.set($key, $value);
        )*
        dict
    }};
}

#[doc(inline)]
pub use crate::__pdf_dictionary as dictionary;
