//! The single door to the PDF layer (M18 slice 1): every module in
//! `sheaf-doc` and every other crate in the workspace reaches the PDF
//! object, structure, filter and encryption types through here.

pub use lopdf::{
    ByteSource, DecompressError, Dictionary, Document, EncryptionState, EncryptionVersion, Error,
    FileBytes, IncrementalDocument, LazyPdf, Limit, LoadOptions, Object, ObjectId, ObjectStream,
    Permissions, ReadLimits, Result, SaveOptions, Stream, StringFormat, XrefFacts, XrefKind,
    content, decode_text_string, encryption, xref,
};

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
