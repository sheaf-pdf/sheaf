//! The single door to the PDF layer (M18 slices 1 and 8–13): every module in
//! `sheaf-doc` and every other crate in the workspace reaches the PDF
//! object, structure, filter and encryption types through here, and
//! content's (`content`) and fonts' (`fonts`, M18 slice 22).

pub mod content;
mod document;
mod error;
pub mod fonts;
mod reader;
mod stream;
#[cfg(any(test, feature = "fixtures"))]
pub mod test_crypt;
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
pub use reader::{LazyPdf, RawCopies, ReadLimits, XrefFacts, XrefKind};
pub use stream::{StreamExt, deflate, deflate_best, deflate_fast, deflate_options, inflate};

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
