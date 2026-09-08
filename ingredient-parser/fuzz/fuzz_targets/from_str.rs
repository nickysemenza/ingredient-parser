#![no_main]
//! Parsing and source attribution must stay total for arbitrary UTF-8 input.
use ingredient::{IngredientParser, ParseOptions, TraceDetail};
use libfuzzer_sys::fuzz_target;
use std::sync::LazyLock;

static PARSER: LazyLock<IngredientParser> = LazyLock::new(IngredientParser::new);

fuzz_target!(|data: &str| {
    let plain = PARSER.from_str(data);
    let observed = PARSER.parse_line(
        data,
        ParseOptions {
            decomposition: true,
            trace: TraceDetail::None,
        },
    );
    assert_eq!(plain, observed.ingredient);
    if let Some(decomposition) = observed.decomposition {
        assert_eq!(decomposition.source, data);
        let mut end = 0;
        for span in decomposition.spans {
            assert!(span.range.start >= end);
            assert_eq!(data.get(span.range.clone()), Some(span.text.as_str()));
            end = span.range.end;
        }
    }
});
