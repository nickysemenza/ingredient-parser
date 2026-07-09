#![no_main]
//! `scrape` parses untrusted HTML/LD-JSON from arbitrary sites; on any input it
//! must return a Result (Ok/Err), never panic.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: (&str, &str)| {
    let (html, url) = data;
    let _ = recipe_scraper::scrape(html, url);
});
