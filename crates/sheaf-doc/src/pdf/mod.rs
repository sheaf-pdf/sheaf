//! The single door to the PDF layer (M18 slices 1 and 8): every module in
//! `sheaf-doc` and every other crate in the workspace reaches the PDF
//! object, structure, filter and encryption types through here.

mod reader;

pub use lopdf::{
    DecompressError, Dictionary, Document, EncryptionState, EncryptionVersion, Error,
    IncrementalDocument, Limit, LoadOptions, Object, ObjectId, ObjectStream, Permissions, Result,
    SaveOptions, Stream, StringFormat, content, decode_text_string, encryption, xref,
};
pub use pdf_structure::{ByteSource, FileBytes};
pub use reader::{LazyPdf, ReadLimits, XrefFacts, XrefKind};

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
