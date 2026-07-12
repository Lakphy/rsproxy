#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../../crates/rsproxy-rules/tests/support/fuzz_harness.rs"]
mod fuzz_harness;

fuzz_target!(|data: &[u8]| {
    fuzz_harness::exercise(data);
});
