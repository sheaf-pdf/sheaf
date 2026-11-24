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

    /// The readers sheaf had beside these, while both stand: what each
    /// gives against what `decode` gives, over every short string of
    /// bytes and many random ones, and where they differ only as the
    /// slice's design says they do (M18 slice 21). It goes with the old
    /// readers. Its writers' half went with the old writers, once it had
    /// shown every character written as before but the ASCII control
    /// characters other than tab, line feed and carriage return, which
    /// are UTF-16BE now.
    mod old_against_new {
        use super::super::*;

        /// The UTF-16 code units of `rest`, read by `unit`, a last odd
        /// byte left out.
        fn units(rest: &[u8], unit: fn([u8; 2]) -> u16) -> impl Iterator<Item = u16> + '_ {
            rest.as_chunks::<2>().0.iter().map(move |&pair| unit(pair))
        }

        /// Where the PDFium reader may read `bytes` otherwise than
        /// `file::text_string` did, as the design's table has it: a
        /// language escape in UTF-16 and UTF-8, and UTF-8 that isn't
        /// valid.
        fn text_string_moves(bytes: &[u8]) -> bool {
            match bytes {
                [0xFE, 0xFF, rest @ ..] => units(rest, u16::from_be_bytes).any(|u| u == 0x1B),
                [0xFF, 0xFE, rest @ ..] => units(rest, u16::from_le_bytes).any(|u| u == 0x1B),
                [0xEF, 0xBB, 0xBF, rest @ ..] => {
                    rest.contains(&0x1B) || std::str::from_utf8(rest).is_err()
                }
                _ => false,
            }
        }

        /// Where it may read `bytes` otherwise than `file::pdf_text_bytes`
        /// did, as the design's table has it: a string marked UTF-16LE or
        /// UTF-8; without a mark, a byte PDFDocEncoding reads otherwise
        /// than Latin-1; after `FE FF`, a language escape or an odd last
        /// byte.
        fn pdf_text_moves(bytes: &[u8]) -> bool {
            match bytes {
                [0xFF, 0xFE, ..] | [0xEF, 0xBB, 0xBF, ..] => true,
                [0xFE, 0xFF, rest @ ..] => {
                    rest.len() % 2 == 1 || units(rest, u16::from_be_bytes).any(|u| u == 0x1B)
                }
                _ => (bytes.iter()).any(|b| matches!(b, 0x18..=0x1F | 0x7F..=0xA0 | 0xAD)),
            }
        }

        /// What `bytes` read as by the old readers and the new, where the
        /// design says they may differ: `(text_string, pdf_text)`, each
        /// counted where they do.
        fn read(bytes: &[u8], moved: &mut [usize; 2]) {
            let new = decode(bytes);
            let old = crate::file::text_string(bytes);
            if text_string_moves(bytes) {
                moved[0] += usize::from(new != old);
            } else {
                assert_eq!(new, old, "file::text_string of {bytes:02X?}");
            }
            // `file::pdf_text` is this of a string's bytes.
            let old = crate::file::pdf_text_bytes(bytes);
            if pdf_text_moves(bytes) {
                moved[1] += usize::from(new != old);
            } else {
                assert_eq!(new, old, "file::pdf_text_bytes of {bytes:02X?}");
            }
        }

        const MARKS: [&[u8]; 4] = [b"", b"\xFE\xFF", b"\xFF\xFE", b"\xEF\xBB\xBF"];

        #[test]
        fn every_short_string_reads_as_before_but_where_the_design_says() {
            let mut moved = [0usize; 2];
            let mut strings = 0usize;
            // Each mark alone too, and the empty string.
            for mark in MARKS {
                for len in 0..=2 {
                    for n in 0..1u32 << (8 * len) {
                        let tail = &n.to_be_bytes()[4 - len..];
                        read(&[mark, tail].concat(), &mut moved);
                        strings += 1;
                    }
                }
            }
            assert_eq!(strings, 4 * (1 + 256 + 65536));
            eprintln!("of {strings} short strings, read otherwise than before: {moved:?}");
            assert!(moved.iter().all(|&n| n > 0), "{moved:?}");
        }

        /// Bytes a random string is made of half the time: the marks', an
        /// escape's, UTF-8's leading and continuing bytes, surrogates',
        /// and PDFDocEncoding's that aren't Latin-1's.
        const TELLING: [u8; 14] = [
            0x00, 0x1B, 0x41, 0xFE, 0xFF, 0xEF, 0xBB, 0xBF, 0xC3, 0xA9, 0xD8, 0xDC, 0x90, 0xAD,
        ];

        #[test]
        fn random_strings_read_as_before_but_where_the_design_says() {
            let mut rng = fastrand::Rng::with_seed(0x5EAF_0021);
            let mut moved = [0usize; 2];
            for _ in 0..100_000 {
                let mark = MARKS[rng.usize(..MARKS.len())];
                let len = rng.usize(..=64 - mark.len());
                let telling = rng.bool();
                let tail: Vec<u8> = (0..len)
                    .map(|_| match telling {
                        true => TELLING[rng.usize(..TELLING.len())],
                        false => rng.u8(..),
                    })
                    .collect();
                read(&[mark, &tail].concat(), &mut moved);
            }
            eprintln!("of 100,000 random strings, read otherwise than before: {moved:?}");
            assert!(moved.iter().all(|&n| n > 0), "{moved:?}");
        }
    }
}
