//! Any bytes as Op JSON: an envelope that reads, applied to a small
//! document of every kind of thing ops name, committed, undone and
//! redone, each step keeping the state's rules.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    sheaf_doc::source::fuzz::op_json(data);
});
