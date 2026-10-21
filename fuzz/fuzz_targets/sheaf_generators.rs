//! A file's watermarks and backgrounds on any bytes, found as an open
//! finds them: its catalog's `/SheafGenerators` read, its pages' streams'
//! markers, its images and pages bound to what its blocks draw, and each
//! laid out on its first pages.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| sheaf_doc::source::fuzz::sheaf_generators(data));
