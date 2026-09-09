//! Source-first cookbook workspace and local library browser.
use crate::theme;
use eframe::egui::{self, RichText};
use egui_graphs::{
    FruchtermanReingoldWithCenterGravity, FruchtermanReingoldWithCenterGravityState,
    Graph as EguiGraph, GraphView, LayoutForceDirected, SettingsInteraction, SettingsNavigation,
    SettingsStyle, get_layout_state, set_layout_state,
};
use hub_shape::HubLabelNodeShape;
use petgraph::Directed;
use petgraph::stable_graph::{DefaultIx, NodeIndex, StableGraph};
use poll_promise::Promise;
use recipe_epub::{BookMeta, CookbookGuess, CookbookRecipe};
use std::path::{Path, PathBuf};

struct ScannedBook {
    meta: BookMeta,
    is_cookbook: bool,
    cover: Option<Vec<u8>>,
    search_key: String,
}
fn cover_uri(path: &Path) -> String {
    format!("bytes://cover/{}", path.to_string_lossy())
}
type ScanResult = Result<Vec<ScannedBook>, String>;
enum LibraryAction {
    None,
    Rescan,
    Load(String),
}
/// Node circle radius in canvas units. egui_graphs sizes the node label font to
/// the radius, so this also controls label legibility (default 5 is too small).
const NODE_RADIUS: f32 = 14.0;

/// Force-directed spread multiplier (`k_scale`). < 1 packs nodes tighter; the
/// default (1.0) leaves the cookbook graph too sparse for the panel.
const LAYOUT_K_SCALE: f32 = 0.35;

/// A node referenced by at least this many recipes is a "hub" (a building-block
/// recipe like Pastry Cream): drawn larger, colored, and always labeled.
const HUB_MIN_INDEGREE: usize = 2;

/// Simulation steps to run when the graph first opens, so it appears settled
/// instead of visibly animating from the seed positions for several seconds.
const PREWARM_STEPS: u32 = 250;

/// egui_graphs `Graph` specialized to our payload-free directed graph. Node
/// labels carry the recipe title; the node payload is the recipe's index in the
/// loaded `recipes` Vec so a click can jump back to the browser.
pub(super) type RefGraph =
    EguiGraph<usize, (), Directed, DefaultIx, HubLabelNodeShape, egui_graphs::DefaultEdgeShape>;

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
    HubLabelNodeShape,
    egui_graphs::DefaultEdgeShape,
    FruchtermanReingoldWithCenterGravityState,
    LayoutForceDirected<FruchtermanReingoldWithCenterGravity>,
>;

/// The cookbook workspace keeps source inspection and durable review in one place.
pub struct CookbookTab {
    review: super::cookbook_review::ReviewPanel,
    pub(crate) path: String,
    pub(crate) library_dir: Option<PathBuf>,
    pub(crate) cookbooks_only: bool,
    pub(crate) library_grid: bool,
    scan: Option<Promise<ScanResult>>,
    filter_text: String,
    library_covers_registered: bool,
    show_library: bool,
}
impl Default for CookbookTab {
    fn default() -> Self {
        Self {
            review: Default::default(),
            path: String::new(),
            library_dir: None,
            cookbooks_only: true,
            library_grid: false,
            scan: None,
            filter_text: String::new(),
            library_covers_registered: false,
            show_library: true,
        }
    }
}
impl CookbookTab {
    pub fn open_review(&mut self, path: PathBuf) {
        self.review.open(path);
        if let Some(source) = self.review.source_path() {
            self.path = source.to_owned();
        }
        self.show_library = false;
    }
    pub(crate) fn invalidate_graph(&mut self) {
        self.review.invalidate_graph();
    }
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.add_enabled_ui(!self.review.busy(), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Cookbooks").size(24.0).strong());
                ui.add_space(24.0);
                ui.menu_button("Open…", |ui| {
                    if ui.button("Cookbook EPUB…").clicked()
                        && let Some(path) =
                            self.file_dialog().add_filter("EPUB", &["epub"]).pick_file()
                    {
                        self.path = path.to_string_lossy().into_owned();
                        self.show_library = false;
                        self.review.inspect(PathBuf::from(&self.path), ctx.clone());
                        ui.close();
                    }
                    if ui.button("Saved run…").clicked()
                        && let Some(path) =
                            self.file_dialog().add_filter("Run", &["json"]).pick_file()
                    {
                        self.open_review(path);
                        ui.close();
                    }
                    if ui.button("Library folder…").clicked()
                        && let Some(path) = self.file_dialog().pick_folder()
                    {
                        self.start_scan(path, ctx.clone());
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("Inspect path", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.path)
                                .hint_text("Cookbook EPUB path")
                                .desired_width(300.0),
                        );
                        if ui
                            .add_enabled(
                                !self.path.trim().is_empty(),
                                egui::Button::new("Inspect source"),
                            )
                            .clicked()
                        {
                            self.show_library = false;
                            self.review
                                .inspect(PathBuf::from(self.path.trim()), ctx.clone());
                            ui.close();
                        }
                    });
                });
                if self.scan.is_some() {
                    ui.toggle_value(&mut self.show_library, "Library");
                }
            });
        });
        ui.separator();
        if !self.library_covers_registered
            && let Some(Ok(books)) = self.scan.as_ref().and_then(Promise::ready)
        {
            for book in books {
                if let Some(bytes) = &book.cover {
                    ctx.include_bytes(cover_uri(&book.meta.path), bytes.clone());
                }
            }
            self.library_covers_registered = true;
        }
        let mut action = LibraryAction::None;
        if self.show_library && self.scan.is_some() {
            egui::Panel::left("cookbook-library")
                .resizable(true)
                .default_size(250.0)
                .show(ui, |ui| match self.scan.as_ref().and_then(Promise::ready) {
                    None => {
                        ui.spinner();
                        ui.label("Scanning library…");
                    }
                    Some(Err(error)) => {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                    Some(Ok(books)) => {
                        action = show_library(
                            ui,
                            books,
                            &mut self.cookbooks_only,
                            &mut self.filter_text,
                            &mut self.library_grid,
                            !self.review.busy(),
                        );
                    }
                });
        }
        match action {
            LibraryAction::None => {}
            LibraryAction::Rescan => {
                if let Some(dir) = self.library_dir.clone() {
                    self.start_scan(dir, ctx.clone());
                }
            }
            LibraryAction::Load(path) => {
                self.path = path.clone();
                self.show_library = false;
                self.review.inspect(PathBuf::from(path), ctx);
            }
        }
        self.review.show(ui);
    }
    fn file_dialog(&self) -> rfd::FileDialog {
        let dialog = rfd::FileDialog::new();
        match &self.library_dir {
            Some(dir) => dialog.set_directory(dir),
            None => dialog,
        }
    }
    fn start_scan(&mut self, dir: PathBuf, ctx: egui::Context) {
        self.library_dir = Some(dir.clone());
        self.show_library = true;
        self.library_covers_registered = false;
        self.scan = Some(Promise::spawn_thread("scan_library", move || {
            let result = scan_library(&dir);
            ctx.request_repaint();
            result
        }));
    }
}

/// Scan local metadata and covers without invoking an extraction backend.
///
/// Free function rather than closure body so it is reachable without a live
/// egui context; the thread and repaint stay at the call site.
fn scan_library(dir: &std::path::Path) -> ScanResult {
    let mut books: Vec<ScannedBook> = recipe_epub::find_epubs(dir)
        .iter()
        .filter_map(|p| recipe_epub::book_metadata(p).ok())
        .map(|meta| {
            let guess = recipe_epub::classify_by_tags(&meta);
            let search_key = format!("{} {}", meta.title, meta.authors.join(" ")).to_lowercase();
            ScannedBook {
                is_cookbook: guess == CookbookGuess::Yes,
                meta,
                cover: None,
                search_key,
            }
        })
        .collect();

    // Read covers only for confirmed cookbooks (one cover decompress
    // each) — bounds the extra I/O over a large library while still
    // giving the grid its thumbnails.
    for b in &mut books {
        if b.is_cookbook {
            b.cover = recipe_epub::book_cover(&b.meta.path).map(|(bytes, _mime)| bytes);
        }
    }

    // Cookbooks first, then alphabetical by title.
    books.sort_by(|a, b| {
        (!a.is_cookbook, a.meta.title.to_lowercase())
            .cmp(&(!b.is_cookbook, b.meta.title.to_lowercase()))
    });
    Ok(books)
}

/// Render the scanned-library browser: a summary line, the cookbooks-only / AI
/// toggles + re-scan, a search box, and the (filtered) book list. Returns the
/// action the caller should take with `&mut self` (a row click → load, the
/// button → re-scan).
fn show_library(
    ui: &mut egui::Ui,
    books: &[ScannedBook],
    cookbooks_only: &mut bool,
    filter: &mut String,
    grid: &mut bool,
    can_load: bool,
) -> LibraryAction {
    let mut action = LibraryAction::None;
    let cookbook_count = books.iter().filter(|b| b.is_cookbook).count();
    ui.label(
        RichText::new(format!(
            "{} epubs · {cookbook_count} cookbooks",
            books.len()
        ))
        .strong(),
    );
    // Toggles reflow in the narrow sidebar rather than overflowing one row.
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(cookbooks_only, "Cookbooks only");
        // Toggle the cover grid off to reclaim space (falls back to a compact list).
        ui.toggle_value(grid, "Covers")
            .on_hover_text("Show cover thumbnails (off = compact list)");
        if ui
            .button("Re-scan")
            .on_hover_text("Re-read the same library directory")
            .clicked()
        {
            action = LibraryAction::Rescan;
        }
    });
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(filter)
                .hint_text("filter by title or author")
                .desired_width(f32::INFINITY),
        );
    });

    let needle = filter.trim().to_lowercase();
    // Books passing the cookbooks-only + free-text filters, in display order.
    let visible = || {
        books.iter().filter(|b| {
            if *cookbooks_only && !b.is_cookbook {
                return false;
            }
            if needle.is_empty() {
                return true;
            }
            b.search_key.contains(&needle)
        })
    };

    // Fill the remaining sidebar height; the panel bounds the scroll region.
    ui.add_enabled_ui(can_load, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if *grid {
                    // A wrapped grid of cover cards (cover thumbnail above the title).
                    ui.horizontal_wrapped(|ui| {
                        for b in visible() {
                            if book_card(ui, b).clicked() {
                                action =
                                    LibraryAction::Load(b.meta.path.to_string_lossy().into_owned());
                            }
                        }
                    });
                } else {
                    // A compact one-line-per-book list (the space-saving view).
                    for b in visible() {
                        let label = if b.meta.authors.is_empty() {
                            b.meta.title.clone()
                        } else {
                            format!("{} — {}", b.meta.title, b.meta.authors.join(", "))
                        };
                        let resp = ui.selectable_label(false, label).on_hover_text(
                            if b.meta.subjects.is_empty() {
                                "no tags".to_string()
                            } else {
                                b.meta.subjects.join(", ")
                            },
                        );
                        if resp.clicked() {
                            action =
                                LibraryAction::Load(b.meta.path.to_string_lossy().into_owned());
                        }
                    }
                }
            });
    });
    action
}

/// Fixed cover thumbnail size for the library grid (book-cover ~3:4 aspect).
const COVER_W: f32 = 96.0;
const COVER_H: f32 = 132.0;

/// One clickable book card in the library grid: a cover thumbnail (or a
/// placeholder for cover-less books) above the truncated title. Returns the
/// card's click response.
fn book_card(ui: &mut egui::Ui, b: &ScannedBook) -> egui::Response {
    let inner = ui.allocate_ui(egui::vec2(COVER_W, COVER_H + 32.0), |ui| {
        ui.set_width(COVER_W);
        ui.vertical_centered(|ui| {
            if b.cover.is_some() {
                ui.add(
                    egui::Image::from_uri(cover_uri(&b.meta.path))
                        .fit_to_exact_size(egui::vec2(COVER_W, COVER_H))
                        .corner_radius(4.0),
                );
            } else {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(COVER_W, COVER_H), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 4.0, ui.visuals().faint_bg_color);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "EPUB",
                    egui::FontId::proportional(28.0),
                    ui.visuals().weak_text_color(),
                );
            }
            ui.add(egui::Label::new(RichText::new(&b.meta.title).small()).truncate());
        });
    });
    inner
        .response
        .interact(egui::Sense::click())
        .on_hover_text(if b.meta.subjects.is_empty() {
            b.meta.title.clone()
        } else {
            format!("{} · {}", b.meta.title, b.meta.subjects.join(", "))
        })
}

pub(crate) struct ReferenceIndex {
    forward: std::collections::HashMap<String, Option<usize>>,
}

impl ReferenceIndex {
    /// Owned so it can be cached with the loaded book — built once per load,
    /// not once (or twice) per frame.
    pub(crate) fn build(recipes: &[CookbookRecipe]) -> Self {
        let mut forward = std::collections::HashMap::new();
        for (i, recipe) in recipes.iter().enumerate() {
            match forward.entry(recipe.meta.title.clone()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Some(i));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
        }
        Self { forward }
    }

    pub(crate) fn resolve(&self, title: &str) -> Option<usize> {
        self.forward.get(title).copied().flatten()
    }
}

pub(super) fn build_reference_graph(recipes: &[CookbookRecipe]) -> RefGraph {
    let index = ReferenceIndex::build(recipes);

    // Collect the directed edges (by recipe index) and the set of participants.
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut participates: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (src, r) in recipes.iter().enumerate() {
        for reference in &r.references {
            if let Some(dst) = index.resolve(&reference.title)
                && src != dst
            {
                edges.push((src, dst));
                participates.insert(src);
                participates.insert(dst);
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
    for (i, nid) in node_ids.into_iter().enumerate() {
        // In-degree = how many recipes use this one. Hubs (the building blocks
        // like Pastry Cream) are the high-in-degree nodes.
        let in_degree = graph
            .g()
            .edges_directed(nid, petgraph::Direction::Incoming)
            .count();
        if let Some(node) = graph.node_mut(nid) {
            let recipe_idx = *node.payload();
            let is_hub = in_degree >= HUB_MIN_INDEGREE;
            // Every node carries its recipe title as the label, so hovering ANY
            // node shows its name. Labels are hover-only (labels_always=false in
            // the view) to avoid a wall of text; hubs still stand out by size +
            // color.
            node.set_label(recipes[recipe_idx].meta.title.clone());
            // Bigger nodes: the default radius (5) makes a tiny hover target and,
            // since the label font size IS the node radius, near-unreadable
            // labels. Hubs (building blocks like Pastry Cream) are drawn larger
            // and tinted so the important recipes stand out.
            node.display_mut().radius = if is_hub {
                NODE_RADIUS * 1.6
            } else {
                NODE_RADIUS
            };
            if is_hub {
                node.set_color(theme::palette().graph_node());
            }
            // Place on a phyllotaxis-style spiral so initial positions are
            // distinct and roughly even — a good seed for force-directed layout.
            let angle = i as f32 * 2.399_963; // golden angle (radians)
            let r = NODE_RADIUS * 1.6 * (i as f32 + 1.0).sqrt();
            node.set_location(egui::Pos2::new(r * angle.cos(), r * angle.sin()));
        }
    }

    // Clear the auto-generated "edge N" labels (default_edge_transform names
    // every edge by index). We don't label edges; "A → B" is conveyed by the
    // arrow plus the two node names.
    let edge_ids: Vec<_> = graph.g().edge_indices().collect();
    for eid in edge_ids {
        if let Some(edge) = graph.edge_mut(eid) {
            edge.set_label(String::new());
        }
    }
    graph
}

/// Render the reference digraph, sync a node click to `selected`, and return
/// `Some(recipe_idx)` if the user clicked the "Open" affordance to jump to that
/// recipe in the Browse view.
pub(super) fn show_reference_graph(
    ui: &mut egui::Ui,
    graph: &mut RefGraph,
    selected: &mut usize,
    prewarmed: &mut bool,
) -> Option<usize> {
    // Tighten the force layout: pull k_scale below the default 1.0 so nodes pack
    // closer (the cookbook graph is otherwise too sparse for the panel). Read
    // the persisted state, override k_scale, write it back.
    let mut state: FruchtermanReingoldWithCenterGravityState = get_layout_state(ui, None);
    if (state.base.k_scale - LAYOUT_K_SCALE).abs() > f32::EPSILON {
        state.base.k_scale = LAYOUT_K_SCALE;
        set_layout_state(ui, state, None);
    }

    // Pre-warm once: advance the simulation headlessly so the graph opens
    // (near-)settled instead of visibly drifting from the seed spiral for
    // several seconds. The remaining per-frame steps just do final polish.
    if !*prewarmed {
        RefGraphView::fast_forward_force_run(ui, graph, PREWARM_STEPS, None);
        *prewarmed = true;
    }

    // Clicking a node selects it; map the selection back to its recipe index
    // and surface a one-click "open" affordance (a graph click selecting a node
    // is more discoverable than expecting the user to also switch tabs).
    let mut clicked_open: Option<usize> = None;
    match graph.selected_nodes().first().copied() {
        Some(nid) => {
            if let Some(node) = graph.node(nid) {
                let recipe_idx = *node.payload();
                *selected = recipe_idx;
                let title = node.label();
                egui::Area::new(egui::Id::new("graph_selection"))
                    .anchor(egui::Align2::LEFT_TOP, egui::vec2(8.0, 8.0))
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&title).strong());
                                if ui.button(format!("Open {}", theme::icon::OPEN)).clicked() {
                                    clicked_open = Some(recipe_idx);
                                }
                            });
                        });
                    });
            }
        }
        // Nothing selected: tell the user how to drive the graph.
        None => {
            egui::Area::new(egui::Id::new("graph_selection"))
                .anchor(egui::Align2::LEFT_TOP, egui::vec2(8.0, 8.0))
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(
                            RichText::new("Click a node to select · drag to pan · scroll to zoom")
                                .weak(),
                        );
                    });
                });
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
        // Per-node label policy lives in HubLabelNodeShape: hubs (which carry a
        // color) are ALWAYS labeled; leaf nodes label on hover/select only. So
        // the view-level labels_always stays false — hover any node for its name
        // without cluttering the canvas with 41 overlapping labels.
        .with_styles(&SettingsStyle::default().with_labels_always(false));
    ui.add(&mut view);

    clicked_open
}

/// A node shape that mirrors egui_graphs' default circle+label, but forces the
/// label to always show for "hub" nodes (those we gave a color in
/// `build_reference_graph`). egui_graphs' `labels_always` is global, so
/// per-node always-on labels need a custom [`DisplayNode`]. Leaf nodes keep the
/// default hover/select-only behavior.
mod hub_shape {
    use eframe::egui::{
        Color32, FontFamily, FontId, Pos2, Shape, Stroke, Vec2,
        epaint::{CircleShape, TextShape},
    };
    use egui_graphs::{DisplayNode, DrawContext, NodeProps};
    use petgraph::{EdgeType, stable_graph::IndexType};

    /// Floor for the label font size (canvas units × zoom). Guards epaint's
    /// `FontId::new(0)` panic when an always-on label is drawn at a tiny zoom.
    const MIN_LABEL_PX: f32 = 6.0;

    #[derive(Clone, Debug)]
    pub struct HubLabelNodeShape {
        pos: Pos2,
        selected: bool,
        dragged: bool,
        hovered: bool,
        color: Option<Color32>,
        label_text: String,
        pub radius: f32,
        /// True when this node should always show its label (set for hubs, which
        /// are the only nodes given a color).
        always_label: bool,
    }

    impl<N: Clone> From<NodeProps<N>> for HubLabelNodeShape {
        fn from(p: NodeProps<N>) -> Self {
            let color = p.color();
            Self {
                pos: p.location(),
                selected: p.selected,
                dragged: p.dragged,
                hovered: p.hovered,
                color,
                label_text: p.label.to_string(),
                radius: 5.0,
                always_label: color.is_some(),
            }
        }
    }

    impl<N: Clone, E: Clone, Ty: EdgeType, Ix: IndexType> DisplayNode<N, E, Ty, Ix>
        for HubLabelNodeShape
    {
        fn is_inside(&self, pos: Pos2) -> bool {
            (pos - self.pos).length() <= self.radius
        }

        fn closest_boundary_point(&self, dir: Vec2) -> Pos2 {
            self.pos + dir.normalized() * self.radius
        }

        fn shapes(&mut self, ctx: &DrawContext) -> Vec<Shape> {
            let center = ctx.meta.canvas_to_screen_pos(self.pos);
            let radius = ctx.meta.canvas_to_screen_size(self.radius);
            let color = self.effective_color(ctx);
            let mut res = vec![
                CircleShape {
                    center,
                    radius,
                    fill: color,
                    stroke: Stroke::default(),
                }
                .into(),
            ];

            // Hubs (always_label) show their label every frame; every other
            // node shows it only while interacted. The view-level
            // `labels_always` is left false, so it isn't consulted here.
            let show_label = self.always_label || self.selected || self.dragged || self.hovered;
            if show_label && !self.label_text.is_empty() {
                let font_px = radius.max(MIN_LABEL_PX);
                let galley = ctx.ctx.fonts_mut(|f| {
                    f.layout_no_wrap(
                        self.label_text.clone(),
                        FontId::new(font_px, FontFamily::Monospace),
                        color,
                    )
                });
                let pos = Pos2::new(center.x - galley.size().x / 2., center.y - radius * 2.);
                res.push(TextShape::new(pos, galley, color).into());
            }
            res
        }

        fn update(&mut self, state: &NodeProps<N>) {
            self.pos = state.location();
            self.selected = state.selected;
            self.dragged = state.dragged;
            self.hovered = state.hovered;
            self.label_text = state.label.to_string();
            self.color = state.color();
            self.always_label = self.color.is_some();
        }
    }

    impl HubLabelNodeShape {
        fn effective_color(&self, ctx: &DrawContext) -> Color32 {
            if let Some(c) = self.color {
                return c;
            }
            let style = if self.selected || self.dragged || self.hovered {
                ctx.ctx.global_style().visuals.widgets.active
            } else {
                ctx.ctx.global_style().visuals.widgets.inactive
            };
            style.fg_stroke.color
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe(title: &str, refs: &[&str]) -> CookbookRecipe {
        CookbookRecipe {
            meta: recipe_epub::RecipeMeta {
                title: title.to_string(),
                ..Default::default()
            },
            sections: vec![],
            source: "book.epub".to_string(),
            url: String::new(),
            references: refs
                .iter()
                .map(|t| recipe_epub::RecipeRef {
                    title: (*t).to_string(),
                    line: String::new(),
                    confidence: recipe_epub::RefConfidence::TitleMatch,
                })
                .collect(),
            image: None,
        }
    }

    /// One title→index answer for the three places that used to each scan the
    /// recipe list themselves — two of them inside render functions.
    #[test]
    fn reference_index_resolves_titles() {
        let recipes = [
            recipe("Pie Dough", &[]),
            recipe("Apple Pie", &["Pie Dough"]),
        ];
        let index = ReferenceIndex::build(&recipes);
        assert_eq!(index.resolve("Pie Dough"), Some(0));
        assert_eq!(index.resolve("Apple Pie"), Some(1));
        assert_eq!(index.resolve("Nonexistent"), None);
    }

    /// The extractor deliberately leaves repeated-title references ambiguous.
    /// Keep that safety boundary in the presentation index too: a HashMap's
    /// normal last-write-wins behavior would silently open the wrong recipe.
    #[test]
    fn reference_index_does_not_resolve_duplicate_titles() {
        let recipes = [
            recipe("Sauce", &[]),
            recipe("Sauce", &[]),
            recipe("Pasta", &["Sauce"]),
        ];
        let index = ReferenceIndex::build(&recipes);
        assert_eq!(index.resolve("Sauce"), None);
    }

    /// A self-reference and an unresolvable target both fail to resolve to an
    /// edge; only the real cross-reference does.
    #[test]
    fn reference_index_underpins_graph_edges() {
        let recipes = [
            recipe("Pie Dough", &["Pie Dough"]),
            recipe("Apple Pie", &["Pie Dough", "Missing Recipe"]),
        ];
        let index = ReferenceIndex::build(&recipes);
        // self-reference: resolves, but src == dst so it is not an edge
        assert_eq!(index.resolve("Pie Dough"), Some(0));
        // unresolvable target: no node to point at
        assert_eq!(index.resolve("Missing Recipe"), None);
    }
}
