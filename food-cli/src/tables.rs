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

/// Human tables are interactive only; machine and redirected output stay plain.
pub fn interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}
pub fn terminal_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let width = terminal_size::terminal_size()
        .map(|(w, _)| usize::from(w.0))
        .unwrap_or(100);
    records_table(headers, rows, width)
}
pub fn records_table(headers: &[&str], rows: &[Vec<String>], width: usize) -> String {
    use tabled::settings::Width;
    if rows.is_empty() {
        return "No results.".into();
    }
    let width = width.max(20);
    if headers.len() > 2 && width < 72.max(headers.len() * 18 + 1) {
        return rows
            .iter()
            .map(|row| {
                let mut b = Builder::default();
                for (key, value) in headers.iter().zip(row) {
                    b.push_record([*key, value.as_str()]);
                }
                b.build()
                    .with(Style::rounded())
                    .with(
                        Width::wrap(width)
                            .keep_words(true)
                            .priority(tabled::settings::peaker::PriorityMax::new(false)),
                    )
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    let mut b = Builder::default();
    b.push_record(headers.iter().copied());
    for row in rows {
        b.push_record(row);
    }
    b.build()
        .with(Style::rounded())
        .with(
            Width::wrap(width)
                .keep_words(true)
                .priority(tabled::settings::peaker::PriorityMax::new(false)),
        )
        .to_string()
}
#[cfg(test)]
mod responsive_tests {
    use super::*;
    #[test]
    fn many_columns_stack_on_a_standard_terminal() {
        let headers = [
            "Book", "Model", "Success", "Coverage", "Review", "Attempts", "Cost",
        ];
        let rows = vec![vec!["value".into(); headers.len()]];
        let compact = records_table(&headers, &rows, 80);
        assert!(
            compact
                .lines()
                .any(|line| line.contains("Book") && line.contains("value"))
        );
        let wide = records_table(&headers, &rows, 160);
        assert!(
            wide.lines()
                .any(|line| line.contains("Book") && line.contains("Cost"))
        );
    }
    #[test]
    fn wraps_unicode_and_handles_empty_tables() {
        for width in [40, 80, 160] {
            let text = records_table(
                &["Book", "Cost"],
                &[vec!["Crème brûlée ".repeat(20), "unknown".into()]],
                width,
            );
            assert!(text.contains("unknown"));
            assert!(text.lines().all(|line| line.chars().count() <= width));
        }
        assert_eq!(records_table(&["Book"], &[], 40), "No results.");
    }
}
