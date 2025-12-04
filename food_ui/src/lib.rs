// UI code uses unwrap for display purposes where panics are acceptable
#![allow(clippy::unwrap_used)]

use eframe::{
    egui::{self, Image, RichText, TextFormat, WidgetText},
    epaint::{text::LayoutJob, Color32},
};
use egui_ltreeview::TreeView;
use ingredient::trace::{ParseTrace, TraceNode, TraceOutcome};
use poll_promise::Promise;
use rand::Rng;
use recipe_scraper::{ParsedRecipe, ScrapedRecipe};
use std::sync::Arc;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Recipe,
    Debug,
    Test,
}

struct Wrapper {
    recipe: ScrapedRecipe,
    parsed: ParsedRecipe,
    traces: Vec<ParseTrace>,
}

pub struct MyApp {
    /// `None` when download hasn't started yet.
    promise: Option<Promise<ehttp::Result<Wrapper>>>,
    url: String,
    current_tab: Tab,
    selected_ingredient_idx: Option<usize>,
    // Test tab state
    test_input: String,
    test_trace: Option<ParseTrace>,
    test_result: Option<ingredient::ingredient::Ingredient>,
}

impl Default for MyApp {
    fn default() -> Self {
        Self {
            promise: None,
            url: "https://cooking.nytimes.com/recipes/1022674-chewy-gingerbread-cookies"
                .to_string(),
            current_tab: Tab::Recipe,
            selected_ingredient_idx: None,
            test_input: "2 cups all-purpose flour, sifted".to_string(),
            test_trace: None,
            test_result: None,
        }
    }
}

fn ui_url(ui: &mut egui::Ui, url: &mut String) -> bool {
    let mut trigger_fetch = false;

    ui.horizontal(|ui| {
        ui.label("URL:");
        trigger_fetch |= ui
            .add(egui::TextEdit::singleline(url).desired_width(f32::INFINITY))
            .lost_focus();
    });
    if ui.button("Random NYT").clicked() {
        let mut rng = rand::rng();
        *url = format!(
            "https://cooking.nytimes.com/recipes/{}",
            rng.random_range(10..15000)
        );
        trigger_fetch = true;
    }

    trigger_fetch
}

fn make_rich(i: &ingredient::ingredient::Ingredient) -> WidgetText {
    let amounts: Vec<String> = i.amounts.iter().map(|id| id.to_string()).collect();
    let modifier = match i.modifier.clone() {
        Some(m) => {
            format!(", {m}")
        }
        None => "".to_string(),
    };
    let amount_list = match amounts.len() {
        0 => "n/a ".to_string(),
        _ => format!("{} ", amounts.join(" / ")),
    };
    let name = i.name.clone();
    // return write!(f, "{}{}{}", amount_list, name, modifier);
    // RichText::new(x.to_string()).color(Color32::GOLD)
    let mut job = LayoutJob::default();
    job.append(
        amount_list.as_str(),
        0.0,
        TextFormat {
            color: Color32::GOLD,
            ..Default::default()
        },
    );
    job.append(
        name.as_str(),
        0.0,
        TextFormat {
            color: Color32::LIGHT_BLUE,
            ..Default::default()
        },
    );
    job.append(
        modifier.as_str(),
        0.0,
        TextFormat {
            color: Color32::LIGHT_GRAY,
            ..Default::default()
        },
    );
    WidgetText::LayoutJob(Arc::new(job))
}

fn show_parsed(ui: &mut egui::Ui, parsed: &ParsedRecipe) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            parsed.ingredients.iter().for_each(|x| {
                // ui.label(x.to_string());
                ui.collapsing(make_rich(x), |ui| {
                    ui.label(serde_json::to_string_pretty(&x).unwrap())
                });
            });
        });
        ui.vertical(|ui| {
            parsed.instructions.iter().for_each(|x| {
                ui.horizontal_wrapped(|ui| {
                    ui.group(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        x.iter().for_each(|x| match x {
                            ingredient::rich_text::Chunk::Measure(x) => x.iter().for_each(|x| {
                                ui.label(RichText::new(x.to_string()).color(Color32::GOLD));
                            }),
                            ingredient::rich_text::Chunk::Text(t) => {
                                ui.label(t);
                            }
                            ingredient::rich_text::Chunk::Ing(i) => {
                                ui.label(RichText::new(i).color(Color32::LIGHT_BLUE));
                            }
                        });
                    });
                });
            });
        });
    });
}

fn show_raw(ui: &mut egui::Ui, recipe: &ScrapedRecipe) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            recipe.ingredients.iter().for_each(|x| {
                ui.label(x);
            });
        });
        ui.vertical(|ui| {
            recipe.instructions.iter().for_each(|x| {
                ui.label(x);
            });
        });
    });
}
impl MyApp {
    /// Called once before the first frame.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Default::default()
    }
}
#[cfg(target_arch = "wasm32")]
fn rewrite_url(url: &str) -> String {
    format!("https://cors.nicky.workers.dev/?target={}", url)
}
#[cfg(not(target_arch = "wasm32"))]
fn rewrite_url(url: &str) -> String {
    url.to_string()
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top panel with tab bar (always visible)
        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.current_tab, Tab::Test, "🧪 Test Parser");
                ui.separator();
                ui.selectable_value(&mut self.current_tab, Tab::Recipe, "📖 Recipe");
                ui.selectable_value(&mut self.current_tab, Tab::Debug, "🔍 Debug Trace");
            });
        });

        // URL bar panel (only shown for Recipe/Debug tabs)
        if self.current_tab != Tab::Test {
            egui::TopBottomPanel::top("url_panel").show(ctx, |ui| {
                let trigger_fetch = ui_url(ui, &mut self.url);

                if trigger_fetch || self.promise.is_none() {
                    let ctx = ctx.clone();
                    let (sender, promise) = Promise::new();
                    let request = ehttp::Request::get(rewrite_url(&self.url.clone()));
                    ehttp::fetch(request, move |response| {
                        let recipe = response.and_then(parse_response);
                        if let Ok(r) = recipe {
                            let traces: Vec<ParseTrace> = r
                                .ingredients
                                .iter()
                                .map(|ing| {
                                    let parser = ingredient::IngredientParser::new(false);
                                    parser.parse_with_trace(ing).trace
                                })
                                .collect();

                            sender.send(Ok(Wrapper {
                                recipe: r.clone(),
                                parsed: r.parse(),
                                traces,
                            }));
                        } else {
                            sender.send(Err(recipe.err().unwrap()));
                        }
                        ctx.request_repaint();
                    });
                    self.promise = Some(promise);
                };
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| match self.current_tab {
            Tab::Test => {
                show_test_tab(
                    ui,
                    &mut self.test_input,
                    &mut self.test_trace,
                    &mut self.test_result,
                );
            }
            Tab::Recipe => {
                if let Some(promise) = &self.promise {
                    match promise.ready() {
                        None => {
                            ui.spinner();
                        }
                        Some(Err(err)) => {
                            ui.colored_label(ui.visuals().error_fg_color, err);
                        }
                        Some(Ok(w)) => {
                            ui.horizontal(|ui| {
                                ui.set_min_height(200.0);
                                ui.heading(w.recipe.name.clone());
                                if let Some(image) = &w.recipe.image {
                                    ui.add(Image::from_uri(image));
                                }
                            });
                            ui.separator();
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                show_parsed(ui, &w.parsed);
                                ui.separator();
                                show_raw(ui, &w.recipe);
                            });
                        }
                    }
                }
            }
            Tab::Debug => {
                if let Some(promise) = &self.promise {
                    match promise.ready() {
                        None => {
                            ui.spinner();
                        }
                        Some(Err(err)) => {
                            ui.colored_label(ui.visuals().error_fg_color, err);
                        }
                        Some(Ok(w)) => {
                            show_debug_tab(ui, &w.traces, &mut self.selected_ingredient_idx);
                        }
                    }
                }
            }
        });
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len).collect();
        format!("{truncated}...")
    }
}

fn show_test_tab(
    ui: &mut egui::Ui,
    test_input: &mut String,
    test_trace: &mut Option<ParseTrace>,
    test_result: &mut Option<ingredient::ingredient::Ingredient>,
) {
    ui.heading("Test Ingredient Parser");
    ui.separator();

    // Input section
    ui.horizontal(|ui| {
        ui.label("Ingredient:");
        let response = ui.add(
            egui::TextEdit::singleline(test_input)
                .desired_width(400.0)
                .hint_text("e.g., 2 cups flour, sifted"),
        );

        if ui.button("Parse").clicked()
            || response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            let parser = ingredient::IngredientParser::new(false);
            let result = parser.parse_with_trace(test_input);
            *test_trace = Some(result.trace);
            *test_result = result.result.ok();
        }

        // Export to Jaeger button (only enabled when trace exists)
        ui.add_enabled_ui(test_trace.is_some(), |ui| {
            if ui.button("📤 Copy Jaeger JSON").clicked() {
                if let Some(trace) = test_trace {
                    let jaeger_json = trace.to_jaeger_json();
                    ui.ctx().copy_text(jaeger_json);
                }
            }
        });
    });

    ui.separator();

    // Results section - 1/3 left, 2/3 right layout
    let available_width = ui.available_width();
    let available_height = ui.available_height();
    let left_width = available_width / 3.0;
    let right_width = available_width * 2.0 / 3.0 - 10.0; // -10 for spacing

    ui.horizontal_top(|ui| {
        ui.set_min_height(available_height);
        // Left column: Parsed result (1/3 width)
        ui.allocate_ui_with_layout(
            egui::vec2(left_width, available_height),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.heading("Parsed Result");
                ui.separator();
                if let Some(result) = test_result {
                    ui.label(format!("Name: {}", result.name));
                    ui.label(format!(
                        "Amounts: {}",
                        result
                            .amounts
                            .iter()
                            .map(|a| a.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                    ui.label(format!(
                        "Modifier: {}",
                        result.modifier.as_deref().unwrap_or("(none)")
                    ));
                    ui.separator();
                    ui.label("JSON:");
                    egui::ScrollArea::vertical()
                        .id_salt("result_json")
                        .max_height(200.0)
                        .show(ui, |ui| {
                            let json = serde_json::to_string_pretty(result).unwrap_or_default();
                            ui.add(egui::TextEdit::multiline(&mut json.as_str()).code_editor());
                        });
                } else {
                    ui.label("Enter an ingredient and click Parse");
                }
            },
        );

        ui.separator();

        // Right column: Parse trace tree (2/3 width)
        ui.allocate_ui_with_layout(
            egui::vec2(right_width, available_height),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.heading("Parse Trace");
                ui.separator();
                if let Some(trace) = test_trace {
                    egui::ScrollArea::vertical()
                        .id_salt("test_trace_tree")
                        .show(ui, |ui| {
                            show_trace_tree(ui, trace);
                        });
                } else {
                    ui.label("Parse trace will appear here");
                }
            },
        );
    });
}

fn show_debug_tab(ui: &mut egui::Ui, traces: &[ParseTrace], selected: &mut Option<usize>) {
    // Use columns for better layout
    ui.columns(2, |columns| {
        // Left column: ingredient selector
        columns[0].heading("Ingredients");
        columns[0].separator();
        egui::ScrollArea::vertical()
            .id_salt("ingredient_list")
            .show(&mut columns[0], |ui| {
                for (idx, trace) in traces.iter().enumerate() {
                    let is_selected = *selected == Some(idx);
                    if ui
                        .selectable_label(is_selected, truncate_str(&trace.input, 50))
                        .clicked()
                    {
                        *selected = Some(idx);
                    }
                }
            });

        // Right column: tree view for selected ingredient
        columns[1].heading("Parse Trace");
        columns[1].separator();
        if let Some(idx) = selected {
            if let Some(trace) = traces.get(*idx) {
                columns[1].label(format!("Input: \"{}\"", trace.input));
                columns[1].separator();
                egui::ScrollArea::vertical()
                    .id_salt("trace_tree")
                    .show(&mut columns[1], |ui| {
                        show_trace_tree(ui, trace);
                    });
            }
        } else {
            columns[1].label("Select an ingredient to view its parse trace");
        }
    });
}

fn show_trace_tree(ui: &mut egui::Ui, trace: &ParseTrace) {
    let id = ui.make_persistent_id("parse_trace_tree");
    TreeView::new(id).show(ui, |builder| {
        render_trace_node(builder, &trace.root, 0);
    });
}

fn render_trace_node(
    builder: &mut egui_ltreeview::TreeViewBuilder<usize>,
    node: &TraceNode,
    id: usize,
) -> usize {
    let label = format_node_label_rich(node);

    if node.children.is_empty() {
        builder.leaf(id, label);
        id + 1
    } else {
        builder.dir(id, label);
        let mut next_id = id + 1;
        for child in &node.children {
            next_id = render_trace_node(builder, child, next_id);
        }
        builder.close_dir();
        next_id
    }
}

fn format_node_label_rich(node: &TraceNode) -> RichText {
    let text = format!(
        "{} \"{}\"{}",
        node.name,
        truncate_str(&node.input, 25),
        match &node.outcome {
            TraceOutcome::Success { output_preview, .. } =>
                format!(" -> {}", truncate_str(output_preview, 20)),
            TraceOutcome::Incomplete => " ...".to_string(),
            _ => String::new(),
        }
    );

    match &node.outcome {
        TraceOutcome::Success { .. } => {
            // Bright green for success path
            RichText::new(text)
                .color(Color32::from_rgb(100, 200, 100))
                .strong()
        }
        TraceOutcome::Failure { .. } => {
            // Muted red for failed branches
            RichText::new(text).color(Color32::from_rgb(180, 90, 90))
        }
        TraceOutcome::Incomplete => RichText::new(text).color(Color32::from_rgb(180, 180, 100)),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn parse_response(response: ehttp::Response) -> Result<ScrapedRecipe, String> {
    match recipe_scraper::scrape(response.text().unwrap(), &response.url) {
        Ok(r) => Ok(r),
        Err(x) => Err(format!("failed to get recipe {x:?}")),
    }
}
