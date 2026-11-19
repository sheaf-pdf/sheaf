//! The single door to the PDF layer (M18 slices 1 and 8): every module in
//! `sheaf-doc` and every other crate in the workspace reaches the PDF
//! object, structure, filter and encryption types through here.

mod document;
mod reader;

pub use document::{Document, EncryptionVersion};
pub use lopdf::{
    DecompressError, Dictionary, EncryptionState, Error, Limit, Object, ObjectId, Permissions,
    Result, Stream, StringFormat, content, encryption,
};
pub use pdf_structure::{ByteSource, FileBytes};
pub use reader::{LazyPdf, RawCopies, ReadLimits, XrefFacts, XrefKind};

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
