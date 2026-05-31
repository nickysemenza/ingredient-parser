#![no_main]
//! `RichParser::parse` returns a Result; on any input it must not panic.
use ingredient::rich_text::RichParser;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    let parser = RichParser::new(["flour", "sugar", "salt"]);
    let _ = parser.parse(data);
});
