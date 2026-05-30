#![no_main]
//! `from_str` is documented to never fail — it must never panic on any input.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    let _ = ingredient::from_str(data);
});
