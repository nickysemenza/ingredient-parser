//! Cookbook tab: load a local EPUB cookbook and browse its AI-extracted
//! recipes. Native-only — `recipe-epub` pulls file/network/tokio deps that
//! don't build for wasm, so the whole module is gated out of the web build.

use eframe::egui::{self, Color32, RichText};
use egui_graphs::{
    DefaultNodeShape, FruchtermanReingoldWithCenterGravity,
    FruchtermanReingoldWithCenterGravityState, Graph as EguiGraph, GraphView, LayoutForceDirected,
    SettingsInteraction, SettingsNavigation, SettingsStyle,
};
use petgraph::stable_graph::{DefaultIx, NodeIndex, StableGraph};
use petgraph::Directed;
use poll_promise::Promise;
use recipe_epub::{CookbookRecipe, ExtractionStats, Options};

use super::recipe::show_parsed_sections;

type LoadResult = Result<(Vec<CookbookRecipe>, ExtractionStats), String>;

/// egui_graphs `Graph` specialized to our payload-free directed graph. Node
/// labels carry the recipe title; the node payload is the recipe's index in the
/// loaded `recipes` Vec so a click can jump back to the browser.
type RefGraph = EguiGraph<usize, (), Directed, DefaultIx>;

/// Force-directed `GraphView` with center gravity — clusters the "building
/// block" recipes (Pie Dough, Pastry Cream, …) toward the center as hubs. The
/// center-gravity term is essential here: the cookbook graph is many
/// *disconnected* clusters (each hub + its dependents), and plain
/// Fruchterman-Reingold has no attraction between components, so repulsion
/// pushes them apart forever (the graph shrinks toward invisible as
/// fit-to-screen keeps zooming out). Gravity keeps it bounded.
type RefGraphView<'a> = GraphView<
    'a,
    usize,
    (),
    Directed,
    DefaultIx,
    DefaultNodeShape,
    egui_graphs::DefaultEdgeShape,
    FruchtermanReingoldWithCenterGravityState,
    LayoutForceDirected<FruchtermanReingoldWithCenterGravity>,
>;

/// State for the Cookbook (EPUB) tab.
#[derive(Default)]
pub struct CookbookTab {
    path: String,
    no_cache: bool,
    /// `None` until a load is started.
    promise: Option<Promise<LoadResult>>,
    selected: usize,
    /// Whether the central panel shows the reference digraph vs the browser.
    show_graph: bool,
    /// The reference digraph, rebuilt only when the loaded book changes.
    /// `graph_nodes[node_index] = recipe index` for click-to-select.
    graph: Option<RefGraph>,
}

impl CookbookTab {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("EPUB:");
            ui.add(
                egui::TextEdit::singleline(&mut self.path)
                    .hint_text("/path/to/cookbook.epub")
                    .desired_width(440.0),
            );
            ui.checkbox(&mut self.no_cache, "No cache");
            let can_load = !self.path.trim().is_empty();
            if ui
                .add_enabled(can_load, egui::Button::new("Load"))
                .clicked()
            {
                self.start_load(ui.ctx().clone());
            }
        });
        ui.label(
            RichText::new(
                "Default model gemini-2.5-flash. Reads AI_GATEWAY_API_KEY (+ ANTHROPIC_BASE_URL \
                 gateway) or OPENAI_API_KEY / ANTHROPIC_API_KEY from the environment. First \
                 load runs LLM extraction (cached afterwards).",
            )
            .weak()
            .small(),
        );
        ui.separator();

        let Some(promise) = &self.promise else {
            ui.label("Enter the path to a .epub file and click Load.");
            return;
        };

        match promise.ready() {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Extracting recipes…");
                });
            }
            Some(Err(err)) => {
                ui.colored_label(ui.visuals().error_fg_color, err);
            }
            Some(Ok((recipes, _))) if recipes.is_empty() => {
                ui.label("No recipes found in this EPUB.");
            }
            Some(Ok((recipes, stats))) => {
                // Disjoint field borrows: `recipes` from self.promise (shared);
                // the rest from distinct `self` fields. Bind each before any
                // closure so no closure captures all of `self`.
                let selected = &mut self.selected;
                let show_graph = &mut self.show_graph;
                let graph_slot = &mut self.graph;
                if *selected >= recipes.len() {
                    *selected = 0;
                }
                let ref_count: usize = recipes.iter().map(|r| r.references.len()).sum();

                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} recipes", recipes.len())).weak());
                    ui.separator();
                    let cost = stats
                        .cost_usd()
                        .map_or_else(|| "cost n/a".to_string(), |c| format!("~${c:.4}"));
                    ui.label(
                        RichText::new(format!(
                            "{cost} · {}/{} chunks cached",
                            stats.chunks_cached, stats.chunks_total
                        ))
                        .weak()
                        .small(),
                    )
                    .on_hover_text(format!(
                        "model: {}\n{} input tok\n{} output tok\n{} cache-read tok\n{} cache-write tok",
                        stats.model,
                        stats.usage.input_tokens,
                        stats.usage.output_tokens,
                        stats.usage.cache_read_input_tokens,
                        stats.usage.cache_creation_input_tokens,
                    ));
                    ui.separator();
                    // Browse | Graph toggle. The graph is only useful when there
                    // are references to draw.
                    ui.selectable_value(show_graph, false, "Browse");
                    ui.add_enabled_ui(ref_count > 0, |ui| {
                        ui.selectable_value(show_graph, true, "Graph")
                            .on_disabled_hover_text("no cross-recipe references in this book");
                    });
                    if ref_count > 0 {
                        ui.label(
                            RichText::new(format!("{ref_count} references"))
                                .weak()
                                .small(),
                        );
                    }
                });

                egui::Panel::left("cookbook_list")
                    .resizable(true)
                    .default_size(240.0)
                    .show_inside(ui, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for (i, r) in recipes.iter().enumerate() {
                                ui.selectable_value(selected, i, &r.meta.title);
                            }
                        });
                    });
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    if *show_graph {
                        // Build the graph lazily; rebuild when the recipe set
                        // changes (node count won't line up otherwise).
                        if graph_slot.is_none() {
                            *graph_slot = Some(build_reference_graph(recipes));
                        }
                        if let Some(graph) = graph_slot {
                            show_reference_graph(ui, graph, selected);
                        }
                    } else {
                        show_recipe_detail(ui, &recipes[*selected]);
                    }
                });
            }
        }
    }

    fn start_load(&mut self, ctx: egui::Context) {
        let path = self.path.trim().to_string();
        let no_cache = self.no_cache;
        self.selected = 0;
        self.graph = None; // rebuilt for the newly loaded book
        self.promise = Some(Promise::spawn_thread("scrape_epub", move || {
            let result = (|| -> LoadResult {
                let bytes = std::fs::read(&path).map_err(|e| format!("read {path}: {e}"))?;
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| format!("tokio runtime: {e}"))?;
                let opts = Options {
                    use_cache: !no_cache,
                    ..Default::default()
                };
                rt.block_on(recipe_epub::extract_cookbook(&bytes, &path, &opts))
                    .map_err(|e| e.to_string())
            })();
            // Wake the UI thread when extraction finishes (poll-promise doesn't
            // repaint on its own).
            ctx.request_repaint();
            result
        }));
    }
}

fn show_recipe_detail(ui: &mut egui::Ui, r: &CookbookRecipe) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading(&r.meta.title);
        if let Some(category) = &r.meta.category {
            ui.label(RichText::new(category).italics().weak());
        }

        ui.horizontal_wrapped(|ui| {
            if let Some(y) = &r.meta.recipe_yield {
                ui.label(RichText::new(format!("📊 {y}")).color(Color32::GOLD));
            }
            if let Some(t) = &r.meta.times {
                for (label, value) in [
                    ("active", &t.active),
                    ("total", &t.total),
                    ("prep", &t.prep),
                    ("cook", &t.cook),
                ] {
                    if let Some(value) = value {
                        ui.label(format!("⏱ {label}: {value}"));
                    }
                }
            }
        });

        if let Some(description) = &r.meta.description {
            ui.add_space(4.0);
            ui.label(RichText::new(description).italics());
        }

        ui.separator();
        // Parse this recipe's verbatim lines with the core nom parser for the
        // color-coded view (cheap — one recipe per frame).
        let parsed = r.parse();
        show_parsed_sections(ui, &parsed.sections);

        if !r.meta.equipment.is_empty() || !r.meta.notes.is_empty() {
            ui.separator();
            for e in &r.meta.equipment {
                ui.label(format!("🔧 {e}"));
            }
            for n in &r.meta.notes {
                ui.label(RichText::new(format!("📝 {n}")).weak());
            }
        }

        // Other recipes in this book that this one uses as ingredients.
        if !r.references.is_empty() {
            ui.separator();
            let targets: Vec<&str> = r.references.iter().map(|x| x.title.as_str()).collect();
            ui.label(RichText::new(format!("↳ Uses recipes: {}", targets.join(", "))).strong());
        }

        ui.add_space(8.0);
        ui.label(RichText::new(&r.url).weak().small());
    });
}

/// Build the recipe-reference digraph: a node per recipe that participates in at
/// least one reference (uses or is used), and a directed edge A→B for every
/// "recipe A uses recipe B" reference. Node payload = the recipe's index in
/// `recipes` (for click-to-select); node label = the recipe title.
fn build_reference_graph(recipes: &[CookbookRecipe]) -> RefGraph {
    // Title → recipe index, to resolve each reference's target title to a node.
    let title_to_idx: std::collections::HashMap<&str, usize> = recipes
        .iter()
        .enumerate()
        .map(|(i, r)| (r.meta.title.as_str(), i))
        .collect();

    // Collect the directed edges (by recipe index) and the set of participants.
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut participates: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (src, r) in recipes.iter().enumerate() {
        for reference in &r.references {
            if let Some(&dst) = title_to_idx.get(reference.title.as_str()) {
                if src != dst {
                    edges.push((src, dst));
                    participates.insert(src);
                    participates.insert(dst);
                }
            }
        }
    }

    // Build a petgraph StableGraph keyed by recipe index, then convert. Only
    // participating recipes get a node so the canvas isn't a field of lone dots.
    let mut sg: StableGraph<usize, (), Directed> = StableGraph::new();
    let mut node_for: std::collections::HashMap<usize, NodeIndex> =
        std::collections::HashMap::new();
    for &recipe_idx in &participates {
        let n = sg.add_node(recipe_idx);
        node_for.insert(recipe_idx, n);
    }
    for (src, dst) in edges {
        // Unwraps safe: both endpoints were inserted into `participates` above.
        sg.add_edge(node_for[&src], node_for[&dst], ());
    }

    // Convert to an egui_graphs Graph, then set each node's label (its title)
    // AND seed a spread-out starting position. egui_graphs' default node
    // transform only sets the label — it leaves every node at the origin
    // (its doc-comment claiming a "random location" is wrong). The random
    // layout seeds positions itself, but the force-directed layout we use only
    // *steps* from existing positions: coincident nodes produce zero repulsion,
    // never separate, and the zero-size bounds make fit-to-screen zoom to
    // infinity (one giant circle). Seeding distinct positions fixes that.
    let mut graph = RefGraph::from(&sg);
    let node_ids: Vec<NodeIndex> = graph.g().node_indices().collect();
    let n = node_ids.len().max(1) as f32;
    for (i, nid) in node_ids.into_iter().enumerate() {
        if let Some(node) = graph.node_mut(nid) {
            let recipe_idx = *node.payload();
            node.set_label(recipes[recipe_idx].meta.title.clone());
            // Place on a phyllotaxis-style spiral so initial positions are
            // distinct and roughly even — a good seed for force-directed layout.
            let angle = i as f32 * 2.399_963; // golden angle (radians)
            let r = 30.0 * (i as f32 / n).sqrt() * n.sqrt();
            node.set_location(egui::Pos2::new(r * angle.cos(), r * angle.sin()));
        }
    }
    graph
}

/// Render the reference digraph and sync a node click back to `selected` so
/// switching to Browse lands on the clicked recipe.
fn show_reference_graph(ui: &mut egui::Ui, graph: &mut RefGraph, selected: &mut usize) {
    // A click selects a node; map it back to the recipe index.
    if let Some(nid) = graph.selected_nodes().first().copied() {
        if let Some(node) = graph.node(nid) {
            *selected = *node.payload();
        }
    }

    let mut view = RefGraphView::new(graph)
        .with_interactions(
            &SettingsInteraction::default()
                .with_dragging_enabled(true)
                .with_node_clicking_enabled(true)
                .with_node_selection_enabled(true),
        )
        .with_navigations(
            // Fit ONCE (egui_graphs fits on the first frame regardless of this
            // flag) then hand control to zoom/pan. Continuous fit-to-screen
            // re-zooms every frame while the layout animates, which is what made
            // the graph shrink toward invisible.
            &SettingsNavigation::default()
                .with_fit_to_screen_enabled(false)
                .with_zoom_and_pan_enabled(true),
        )
        // Labels only on hover/select/drag — NOT always. `with_labels_always`
        // forces a text galley for every node on the first frame, before the
        // layout + fit-to-screen settle the zoom; a sub-pixel node then renders
        // a 0px font and epaint panics ("Bad px_scale_factor: 0"). On hover the
        // node is at interaction scale, so the font size is always > 0. It's
        // also far more legible than 41 overlapping labels.
        .with_styles(&SettingsStyle::default().with_labels_always(false));
    ui.add(&mut view);
}
