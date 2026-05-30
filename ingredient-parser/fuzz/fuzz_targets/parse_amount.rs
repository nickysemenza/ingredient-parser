#![no_main]
//! `parse_amount` returns a Result; on any input it must return (Ok/Err), never panic.
use ingredient::IngredientParser;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    let parser = IngredientParser::new();
    let _ = parser.parse_amount(data);
});
