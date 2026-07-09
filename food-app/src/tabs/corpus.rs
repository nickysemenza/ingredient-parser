//! Corpus QA tab: load `tests/corpus/corpus.jsonl`, re-run the parser on each
//! row, and score it exactly the way `tests/accuracy.rs` does — so the GUI is a
//! live mirror of the regression ratchet.
//!
//! Status per row (mirrors `accuracy_corpus`'s classification):
//! - **Exact**     — all labeled fields match (a committed row that passes).
//! - **Regression**— a committed (non-xfail) row that mismatches. This is what
//!   the test would *fail* on.
//! - **Xfail**     — a known-gap row (`xfail` set) that still mismatches.
//! - **Promote**   — an xfail row that now matches; the `xfail` marker can go.
//!
//! Field comparison reuses the `ingredient` crate types (`from_str`,
//! `Measure`, `IngredientUsage`) with the same equality accuracy.rs uses —
//! nothing is reimplemented.

use crate::theme;
use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};
use ingredient::ingredient::Ingredient;
use ingredient::util::truncate_str;
use ingredient::{IngredientUsage, from_str, unit::Measure};
use serde::Deserialize;

/// Default corpus path, relative to the workspace root (the app's cwd under
/// `cargo run --bin food-app`). Editable in the tab's path field.
const DEFAULT_CORPUS_PATH: &str = "ingredient-parser/tests/corpus/corpus.jsonl";

/// Baked-in copy of the corpus, used as a fallback when the file can't be read
/// (e.g. the app was launched from a different cwd). Runtime load is preferred
/// so edits to the file show up without a rebuild; this only backstops IO.
const EMBEDDED_CORPUS: &str = include_str!("../../../ingredient-parser/tests/corpus/corpus.jsonl");

/// One labeled corpus row — the same serde shape as `accuracy.rs::CorpusRow`.
#[derive(Deserialize)]
struct CorpusRow {
    input: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    amounts: Vec<Measure>,
    #[serde(default)]
    modifier: Option<String>,
    #[serde(default)]
    optional: bool,
    /// Expected usage; absent means `Normal` (a test-side default, matching
    /// accuracy.rs — the `Ingredient.usage` field itself has no serde default).
    #[serde(default)]
    usage: IngredientUsage,
    /// When set, documents a known parser gap: a mismatch is tolerated.
    #[serde(default)]
    xfail: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Exact,
    Regression,
    Xfail,
    Promote,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Exact => "EXACT",
            Self::Regression => "REGRESSION",
            Self::Xfail => "XFAIL",
            Self::Promote => "PROMOTE",
        }
    }

    fn color(self) -> egui::Color32 {
        let p = theme::palette();
        match self {
            Self::Exact => p.trace_ok(),
            Self::Regression => p.trace_fail(),
            Self::Xfail => p.trace_incomplete(),
            Self::Promote => p.amount(),
        }
    }
}

/// One field's expected-vs-got comparison, for the detail view.
struct FieldDiff {
    field: &'static str,
    ok: bool,
    expected: String,
    got: String,
}

/// A scored corpus row: the expected labels, the parser's actual output, and
/// the per-field diff. `got` is retained so the detail view is free.
struct ScoredRow {
    input: String,
    status: Status,
    xfail_reason: Option<String>,
    diffs: Vec<FieldDiff>,
}

impl ScoredRow {
    /// Score one row against the parser, mirroring `accuracy.rs` semantics:
    /// exact equality on every labeled field; `xfail` only changes how a
    /// mismatch is *classified*, never how fields are compared.
    fn score(row: CorpusRow) -> Self {
        let got: Ingredient = from_str(&row.input);
        let diffs = vec![
            FieldDiff {
                field: "name",
                ok: got.name == row.name,
                expected: row.name.clone(),
                got: got.name.clone(),
            },
            FieldDiff {
                field: "amounts",
                ok: got.amounts == row.amounts,
                expected: fmt_amounts(&row.amounts),
                got: fmt_amounts(&got.amounts),
            },
            FieldDiff {
                field: "modifier",
                ok: got.modifier == row.modifier,
                expected: format!("{:?}", row.modifier),
                got: format!("{:?}", got.modifier),
            },
            FieldDiff {
                field: "optional",
                ok: got.optional == row.optional,
                expected: row.optional.to_string(),
                got: got.optional.to_string(),
            },
            FieldDiff {
                field: "usage",
                ok: got.usage == row.usage,
                expected: format!("{:?}", row.usage),
                got: format!("{:?}", got.usage),
            },
        ];

        let all_ok = diffs.iter().all(|d| d.ok);
        let status = match (all_ok, row.xfail.is_some()) {
            (true, false) => Status::Exact,
            (true, true) => Status::Promote,
            (false, false) => Status::Regression,
            (false, true) => Status::Xfail,
        };

        Self {
            input: row.input,
            status,
            xfail_reason: row.xfail,
            diffs,
        }
    }
}

fn fmt_amounts(amounts: &[Measure]) -> String {
    if amounts.is_empty() {
        return "[]".to_string();
    }
    amounts
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Regressions,
    Promotes,
    Xfails,
}

impl Filter {
    fn matches(self, status: Status) -> bool {
        match self {
            Self::All => true,
            Self::Regressions => status == Status::Regression,
            Self::Promotes => status == Status::Promote,
            Self::Xfails => status == Status::Xfail,
        }
    }
}

/// State for the Corpus QA tab.
pub struct CorpusTab {
    /// Path to the corpus file (editable). Persisted across runs.
    pub(crate) path: String,
    rows: Vec<ScoredRow>,
    /// A load error (bad path / malformed JSON), shown inline.
    load_error: Option<String>,
    /// Non-empty once a load succeeds — distinguishes "not loaded yet" from
    /// "loaded, zero rows".
    loaded: bool,
    filter: Filter,
    selected: Option<usize>,
    /// Sorted/filtered view into `rows`, rebuilt on load or filter change.
    order: Vec<usize>,
}

impl Default for CorpusTab {
    fn default() -> Self {
        Self {
            path: DEFAULT_CORPUS_PATH.to_string(),
            rows: Vec::new(),
            load_error: None,
            loaded: false,
            filter: Filter::All,
            selected: None,
            order: Vec::new(),
        }
    }
}

/// What the tab wants the app to do after a frame — currently only "send this
/// input to the Test tab". Returned to `lib.rs`, which owns both tab structs.
pub enum CorpusAction {
    SendToTest(String),
}

impl CorpusTab {
    /// Render the tab. Returns an action for the app to apply (e.g. switch to
    /// the Test tab and prefill its input) since cross-tab state lives on the
    /// app struct, not here.
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<CorpusAction> {
        let mut action = None;

        ui.heading("Corpus QA");
        ui.label(
            RichText::new(
                "Re-runs the parser over the accuracy corpus and scores each row \
                 exactly like tests/accuracy.rs.",
            )
            .weak()
            .small(),
        );
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Corpus path:");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.path)
                    .desired_width(360.0)
                    .font(egui::TextStyle::Monospace),
            );
            // Load on Enter (not bare lost_focus, which also fires on Tab/click-
            // away — same idiom as the Test/Recipe tabs).
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Load").clicked() || enter {
                self.load();
            }
            if ui
                .button("Use embedded")
                .on_hover_text("Load the copy baked into the binary at build time")
                .clicked()
            {
                self.load_from_source(EMBEDDED_CORPUS);
            }
        });

        if let Some(err) = &self.load_error {
            ui.colored_label(theme::palette().trace_fail(), err);
        }

        if !self.loaded {
            ui.add_space(8.0);
            ui.label(RichText::new("Click Load to score the corpus.").weak());
            return action;
        }
        ui.separator();

        self.show_summary(ui);
        ui.separator();

        // Detail panel for the selected row (before the table so the table
        // fills the remaining space).
        if let Some(idx) = self.selected
            && let Some(row) = self.rows.get(idx)
            && let Some(a) = show_detail(ui, row)
        {
            action = Some(a);
        }

        self.show_table(ui);
        action
    }

    /// Load and score the corpus from the file at `self.path`.
    fn load(&mut self) {
        match std::fs::read_to_string(&self.path) {
            Ok(source) => self.load_from_source(&source),
            Err(e) => {
                self.load_error = Some(format!("failed to read {}: {e}", self.path));
                // Keep any previously-loaded rows visible rather than blanking.
            }
        }
    }

    /// Parse and score every row from an already-read corpus source. A single
    /// malformed line aborts the load with an error (matching accuracy.rs,
    /// which panics on a bad row) rather than silently dropping it.
    fn load_from_source(&mut self, source: &str) {
        let mut rows = Vec::new();
        for line in source.lines().map(str::trim) {
            if line.is_empty() || line.starts_with("//") {
                continue;
            }
            match serde_json::from_str::<CorpusRow>(line) {
                Ok(row) => rows.push(ScoredRow::score(row)),
                Err(e) => {
                    self.load_error = Some(format!("invalid corpus row: {line}\n  {e}"));
                    return;
                }
            }
        }
        self.rows = rows;
        self.load_error = None;
        self.loaded = true;
        self.selected = None;
        self.rebuild_order();
    }

    fn rebuild_order(&mut self) {
        self.order = (0..self.rows.len())
            .filter(|&i| self.filter.matches(self.rows[i].status))
            .collect();
        // Drop a stale selection that the new filter hides.
        if let Some(sel) = self.selected
            && !self.order.contains(&sel)
        {
            self.selected = None;
        }
    }

    /// Status counts + filter buttons.
    fn show_summary(&mut self, ui: &mut egui::Ui) {
        let (mut exact, mut regr, mut xfail, mut promote) = (0, 0, 0, 0);
        for r in &self.rows {
            match r.status {
                Status::Exact => exact += 1,
                Status::Regression => regr += 1,
                Status::Xfail => xfail += 1,
                Status::Promote => promote += 1,
            }
        }
        let total = self.rows.len();

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("{total} rows")).strong());
            ui.separator();
            count_chip(ui, Status::Exact, exact);
            count_chip(ui, Status::Regression, regr);
            count_chip(ui, Status::Xfail, xfail);
            count_chip(ui, Status::Promote, promote);
        });

        let mut filter = self.filter;
        ui.horizontal(|ui| {
            ui.label("Filter:");
            ui.selectable_value(&mut filter, Filter::All, format!("All ({total})"));
            ui.selectable_value(
                &mut filter,
                Filter::Regressions,
                format!("Regressions ({regr})"),
            );
            ui.selectable_value(
                &mut filter,
                Filter::Promotes,
                format!("Promotes ({promote})"),
            );
            ui.selectable_value(&mut filter, Filter::Xfails, format!("Xfails ({xfail})"));
        });
        if filter != self.filter {
            self.filter = filter;
            self.rebuild_order();
        }
    }

    fn show_table(&mut self, ui: &mut egui::Ui) {
        let mut clicked = None;
        let selected = self.selected;
        let rows = &self.rows;
        let order = &self.order;

        if order.is_empty() {
            ui.add_space(8.0);
            ui.label(RichText::new("No rows match this filter.").weak());
            return;
        }

        let row_height = egui::TextStyle::Body.resolve(ui.style()).size + 8.0;
        TableBuilder::new(ui)
            .striped(true)
            .sense(egui::Sense::click())
            .column(Column::auto().at_least(100.0))
            .column(Column::remainder().clip(true))
            .header(22.0, |mut header| {
                header.col(|ui| {
                    ui.label(RichText::new("Status").strong());
                });
                header.col(|ui| {
                    ui.label(RichText::new("Input").strong());
                });
            })
            .body(|body| {
                body.rows(row_height, order.len(), |mut table_row| {
                    let idx = order[table_row.index()];
                    let r = &rows[idx];
                    table_row.set_selected(selected == Some(idx));
                    table_row.col(|ui| {
                        ui.label(
                            RichText::new(r.status.label())
                                .color(r.status.color())
                                .strong(),
                        );
                    });
                    table_row.col(|ui| {
                        ui.label(truncate_str(&r.input, 80)).on_hover_text(&r.input);
                    });
                    if table_row.response().clicked() {
                        clicked = Some(idx);
                    }
                });
            });

        if let Some(idx) = clicked {
            // Toggle selection off when re-clicking the open row.
            self.selected = (self.selected != Some(idx)).then_some(idx);
        }
    }
}

/// A small colored status count, e.g. "EXACT 128".
fn count_chip(ui: &mut egui::Ui, status: Status, n: usize) {
    ui.label(
        RichText::new(format!("{} {n}", status.label()))
            .color(status.color())
            .strong(),
    );
}

/// The selected row's expected-vs-got detail, plus a "Send to Test tab" button.
/// Returns a [`CorpusAction`] when that button is clicked.
fn show_detail(ui: &mut egui::Ui, row: &ScoredRow) -> Option<CorpusAction> {
    let mut action = None;
    theme::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(row.status.label())
                    .color(row.status.color())
                    .strong(),
            );
            ui.label(RichText::new(truncate_str(&row.input, 80)).monospace());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("📤 Send to Test tab").clicked() {
                    action = Some(CorpusAction::SendToTest(row.input.clone()));
                }
            });
        });

        if let Some(reason) = &row.xfail_reason {
            ui.label(RichText::new(format!("xfail: {reason}")).italics().weak());
        }
        ui.separator();

        // Only surface the fields that actually differ for a mismatch; for an
        // exact/promote row show all fields (they all match) so the view isn't
        // empty.
        let any_mismatch = row.diffs.iter().any(|d| !d.ok);
        for d in &row.diffs {
            if any_mismatch && d.ok {
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                let color = if d.ok {
                    theme::palette().trace_ok()
                } else {
                    theme::palette().trace_fail()
                };
                ui.label(RichText::new(format!("{}:", d.field)).color(color).strong());
                if d.ok {
                    ui.monospace(&d.got);
                } else {
                    ui.monospace(format!("got {}", d.got));
                    ui.label(RichText::new("·").weak());
                    ui.monospace(format!("want {}", d.expected));
                }
            });
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a `CorpusRow` the same way `load_from_source` does — from a
    /// corpus.jsonl-shaped JSON line — so these tests exercise the exact same
    /// deserialization path as the real corpus file.
    fn row(json: &str) -> CorpusRow {
        serde_json::from_str(json).unwrap()
    }

    /// A committed row (no `xfail`) whose labels match the parser's actual
    /// output scores as `Exact` — mirrors `accuracy_corpus`'s `exact` bucket.
    #[test]
    fn matching_committed_row_is_exact() {
        let scored = ScoredRow::score(row(
            r#"{"input": "1 cup flour", "name": "flour", "amounts": [{"unit": "cup", "value": 1}]}"#,
        ));
        assert!(scored.status == Status::Exact);
        assert!(scored.diffs.iter().all(|d| d.ok));
    }

    /// A committed row (no `xfail`) whose labels DON'T match the parser's
    /// output scores as `Regression` — this is exactly what `accuracy_corpus`
    /// fails the test suite on.
    #[test]
    fn mismatching_committed_row_is_regression() {
        let scored = ScoredRow::score(row(
            r#"{"input": "1 cup flour", "name": "sugar", "amounts": [{"unit": "cup", "value": 1}]}"#,
        ));
        assert!(scored.status == Status::Regression);
        assert!(scored.diffs.iter().any(|d| !d.ok));
    }

    /// A known-gap row (`xfail` set) whose labels still don't match scores as
    /// `Xfail` — the mismatch is tolerated, never a regression.
    #[test]
    fn mismatching_xfail_row_is_xfail() {
        let scored = ScoredRow::score(row(
            r#"{"input": "1 cup flour", "name": "sugar", "amounts": [{"unit": "cup", "value": 1}], "xfail": "known gap"}"#,
        ));
        assert!(scored.status == Status::Xfail);
        assert_eq!(scored.xfail_reason.as_deref(), Some("known gap"));
    }

    /// An `xfail` row whose labels now match the parser's output (the gap has
    /// been closed) scores as `Promote` — the hint to remove the `xfail`
    /// marker, mirroring `accuracy_corpus`'s `promotable` list.
    #[test]
    fn matching_xfail_row_is_promote() {
        let scored = ScoredRow::score(row(
            r#"{"input": "1 cup flour", "name": "flour", "amounts": [{"unit": "cup", "value": 1}], "xfail": "fixed now"}"#,
        ));
        assert!(scored.status == Status::Promote);
        assert!(scored.diffs.iter().all(|d| d.ok));
    }

    /// `xfail` only changes classification, never the underlying field
    /// comparison: an xfail row scored identically to its non-xfail twin
    /// produces the same per-field diffs either way.
    #[test]
    fn xfail_does_not_change_field_comparison() {
        let base =
            r#"{"input": "1 cup flour", "name": "sugar", "amounts": [{"unit": "cup", "value": 1}]"#;
        let committed = ScoredRow::score(row(&format!("{base}}}")));
        let xfailed = ScoredRow::score(row(&format!("{base}, \"xfail\": \"reason\"}}")));

        let committed_oks: Vec<bool> = committed.diffs.iter().map(|d| d.ok).collect();
        let xfailed_oks: Vec<bool> = xfailed.diffs.iter().map(|d| d.ok).collect();
        assert_eq!(committed_oks, xfailed_oks);
    }

    #[test]
    fn filter_matches_each_status() {
        assert!(Filter::All.matches(Status::Exact));
        assert!(Filter::All.matches(Status::Regression));
        assert!(Filter::Regressions.matches(Status::Regression));
        assert!(!Filter::Regressions.matches(Status::Exact));
        assert!(Filter::Promotes.matches(Status::Promote));
        assert!(!Filter::Promotes.matches(Status::Xfail));
        assert!(Filter::Xfails.matches(Status::Xfail));
        assert!(!Filter::Xfails.matches(Status::Regression));
    }
}
