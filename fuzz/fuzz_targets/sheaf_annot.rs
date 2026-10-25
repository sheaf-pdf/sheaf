//! A page's annotations on any bytes, read as sheaf's marks: each
//! `/SHEF_Annot` and its staleness digest, and whether it's sheaf's to
//! rewrite.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| sheaf_doc::source::fuzz::sheaf_annot(data));
