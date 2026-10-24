//! Any bytes as a page's content in two streams, drawn on a page whose
//! resources name a font of each kind, color spaces, property lists and
//! forms, and, after each `%%font` line, the bytes of a font's CMap, a
//! CharProc, a `/W` and a `/FontMatrix`: interpreted, every glyph laid
//! out with its box, and written again from what it draws, which draws
//! the same, everything where it was.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    sheaf_doc::source::fuzz::interp(data);
});
