//! Text strings (ISO 32000-2, 7.9.2.2) through `pdf-objects` (M18 slice
//! 21): the one place sheaf encodes the text it writes into a file and
//! decodes the text it reads from one, so every writer and reader of a
//! title, a name, a field's value or a mark's contents agrees.
//!
//! **Written** ([`string`], [`encoded`]): the text's own bytes where every
//! one is a tab, a line feed, a carriage return or printable ASCII, which
//! PDFDocEncoding reads as themselves; otherwise UTF-16BE after its byte
//! order mark. U+001B is written as it is, in UTF-16BE, where a reader
//! takes it for a language escape.
//!
//! **Read** ([`decode`], [`of`]): as PDFium reads one (chromium/8152), the
//! page view's engine: UTF-16BE after `FE FF`, UTF-16LE after `FF FE`
//! (a last odd byte left out of either), UTF-8 after `EF BB BF` (bytes
//! that aren't UTF-8 read by PDFium's own state machine), each with its
//! language escapes left out; else PDFDocEncoding, NUL for the three
//! bytes it leaves undefined.

use pdf_objects::Rules;

use super::Object;

/// `text` as a PDF string object: its bytes as [`encoded`] gives them,
/// written `(…)` when they are the text's own and `<…>` when UTF-16BE.
#[must_use]
pub fn string(text: &str) -> Object {
    Object::text(text)
}

/// `text`'s bytes as a text string, as [`string`] holds them: for a name
/// tree's key, which is a string with no form of its own.
#[must_use]
pub fn encoded(text: &str) -> Vec<u8> {
    pdf_objects::text::encode(text)
}

/// The text a text string's `bytes` hold, as PDFium reads it.
#[must_use]
pub fn decode(bytes: &[u8]) -> String {
    pdf_objects::text::decode_str(bytes, Rules::Pdfium)
}

/// The text `object` holds, if it's a string ([`decode`]); empty for
/// anything else.
#[must_use]
pub fn of(object: &Object) -> String {
    object.as_bytes().map(decode).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::literal;

    /// `text` in UTF-16BE after its mark, as [`string`] writes what isn't
    /// its own bytes.
    fn utf16be(text: &str) -> Vec<u8> {
        let units = text.encode_utf16().flat_map(u16::to_be_bytes);
        [0xFE, 0xFF].into_iter().chain(units).collect()
    }

    #[test]
    fn a_string_is_its_own_bytes_or_utf16be() {
        let table: [(&str, Object); 9] = [
            ("Hello, world", literal("Hello, world")),
            ("tab\tline\ncr\r", literal("tab\tline\ncr\r")),
            ("", literal("")),
            ("é", Object::hex(vec![0xFE, 0xFF, 0x00, 0xE9])),
            // DEL, which PDFDocEncoding leaves undefined.
            ("\u{7F}", Object::hex(vec![0xFE, 0xFF, 0x00, 0x7F])),
            // A form feed, a control character PDFDocEncoding has none of.
            ("\u{0C}", Object::hex(vec![0xFE, 0xFF, 0x00, 0x0C])),
            // PDFDocEncoding's accents, which aren't ASCII's.
            ("\u{1A}", Object::hex(vec![0xFE, 0xFF, 0x00, 0x1A])),
            ("’", Object::hex(vec![0xFE, 0xFF, 0x20, 0x19])),
            (
                "a😀",
                Object::hex(vec![0xFE, 0xFF, 0x00, 0x61, 0xD8, 0x3D, 0xDE, 0x00]),
            ),
        ];
        for (text, written) in table {
            assert_eq!(string(text), written, "{text:?}");
            assert_eq!(encoded(text), written.as_bytes().unwrap(), "{text:?}");
        }
    }

    #[test]
    fn a_string_reads_as_pdfium_reads_it() {
        let table: [(&[u8], &str); 22] = [
            (b"Hello", "Hello"),
            (b"", ""),
            // PDFDocEncoding: Latin-1's but for its accents, its quotes and
            // dashes, and NUL for the three it leaves undefined.
            (b"caf\xE9", "café"),
            (b"\x90", "’"),
            (b"\x85", "–"),
            (b"\x8D\x8E", "“”"),
            (b"\xA0", "€"),
            (b"\x18\x1F", "˘˜"),
            (b"\x7F", "\0"),
            (b"\x9F", "\0"),
            (b"\xAD", "\0"),
            (b"\x0C\x01", "\u{0C}\u{01}"),
            // UTF-16BE, a last odd byte left out.
            (b"\xFE\xFF\x00A\x00b", "Ab"),
            (b"\xFE\xFF\x00A\x00", "A"),
            (b"\xFE\xFF\xD8\x3D\xDE\x00", "😀"),
            (b"\xFE\xFF\xD8\x3D\x00A", "\u{FFFD}A"),
            // UTF-16LE, which ISO's table doesn't have.
            (b"\xFF\xFEA\x00b\x00", "Ab"),
            // UTF-8, PDF 2.0's; bytes that aren't UTF-8 by PDFium's
            // state machine, which skips a byte that continues nothing.
            (b"\xEF\xBB\xBFcaf\xC3\xA9", "café"),
            (b"\xEF\xBB\xBF\x80A", "A"),
            // A language escape is left out, in UTF-16 and UTF-8 alike.
            (b"\xFE\xFF\x00\x1B\x00e\x00n\x00\x1B\x00H\x00i", "Hi"),
            (b"\xEF\xBB\xBF\x1Ben\x1BHi", "Hi"),
            // An escape not closed leaves out the rest.
            (b"\xFE\xFF\x00H\x00\x1B\x00i", "H"),
        ];
        for (bytes, text) in table {
            assert_eq!(decode(bytes), text, "{bytes:02X?}");
            assert_eq!(of(&literal(bytes)), text, "{bytes:02X?}");
            assert_eq!(of(&Object::hex(bytes)), text, "{bytes:02X?}");
        }
    }

    #[test]
    fn what_isn_t_a_string_holds_no_text() {
        for object in [
            Object::from(1),
            Object::name("Name"),
            Object::null(),
            Object::from(vec![literal("a")]),
        ] {
            assert_eq!(of(&object), "", "{object:?}");
        }
    }

    /// Every character but U+001B, alone and between letters, reads back
    /// as itself: U+001B starts a language escape.
    #[test]
    fn every_character_but_an_escape_reads_back_as_itself() {
        let mut read = 0usize;
        for c in ('\0'..=char::MAX).filter(|&c| c != '\u{1B}') {
            for text in [c.to_string(), format!("a{c}b")] {
                let written = string(&text);
                let bytes = written.as_bytes().unwrap();
                assert_eq!(decode(bytes), text, "{c:?}");
                read += 1;
            }
        }
        assert_eq!(read, 2 * (0x11_0000 - 0x800 - 1));
        assert_eq!(decode(&utf16be("a\u{1B}b")), "a");
    }
}
