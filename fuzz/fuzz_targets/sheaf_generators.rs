//! A file's generators (watermarks, backgrounds, headers and footers) on
//! any bytes, found as an open finds them: its catalog's
//! `/SHEF_Generators` read, its pages' streams' markers and their Bates
//! numbers, its images and pages bound to what its blocks draw, and each
//! laid out on its first pages.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| sheaf_doc::source::fuzz::sheaf_generators(data));
