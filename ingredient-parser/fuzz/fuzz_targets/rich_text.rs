#![no_main]
//! `RichParser::parse` returns a Result; on any input it must not panic.
use ingredient::rich_text::RichParser;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    let parser = RichParser::new(vec![
        "flour".to_string(),
        "sugar".to_string(),
        "salt".to_string(),
    ]);
    let _ = parser.parse(data);
});
