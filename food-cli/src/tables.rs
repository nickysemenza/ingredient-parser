//! Small `tabled` helpers for CLI output. Each returns a rendered `String` so
//! callers keep control of `print!`/`eprintln!` and stay out of the JSON /
//! pipeable code paths.

use ingredient::unit::Measure;
use tabled::{builder::Builder, settings::Style};

/// Render parsed amounts as a value/unit/upper table.
pub fn amount_table(amounts: &[Measure]) -> String {
    let mut b = Builder::default();
    b.push_record(["value", "unit", "upper"]);
    for m in amounts {
        let upper = m
            .upper_value()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string());
        b.push_record([m.value().to_string(), m.unit().to_string(), upper]);
    }
    b.build().with(Style::rounded()).to_string()
}
