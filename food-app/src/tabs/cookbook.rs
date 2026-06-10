//! Cookbook tab: load a local EPUB cookbook and browse its AI-extracted
//! recipes.

use crate::theme;
use eframe::egui::{self, RichText};
use egui_graphs::{
    get_layout_state, set_layout_state, FruchtermanReingoldWithCenterGravity,
    FruchtermanReingoldWithCenterGravityState, Graph as EguiGraph, GraphView, LayoutForceDirected,
    SettingsInteraction, SettingsNavigation, SettingsStyle,
};

use hub_shape::HubLabelNodeShape;
use petgraph::stable_graph::{DefaultIx, NodeIndex, StableGraph};
use petgraph::Directed;
use poll_promise::Promise;
use recipe_epub::{
    BookMeta, CookbookGuess, CookbookRecipe, CookbookRecipeExt, ExtractionStats, ImageRef, Options,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A fully loaded book: its recipes + extraction stats, plus the materialized
/// image bytes (the book cover and every recipe's hero photo) read once off the
/// UI thread. The bytes are registered with egui as `bytes://<archive_path>` URIs
/// the first frame they're observed, then referenced by `Image::from_uri`.
struct LoadedBook {
    recipes: Vec<CookbookRecipe>,
    stats: ExtractionStats,
    /// The book's cover reference (URI key), if any.
    cover: Option<ImageRef>,
    /// De-duplicated `(archive_path, bytes)` for the cover + all heroes.
    images: Vec<(String, Vec<u8>)>,
}

type LoadResult = Result<LoadedBook, String>;

/// A book discovered while scanning a library directory: its metadata, the
/// tag-based guess, the final cookbook verdict (which the AI fallback may override
/// for [`CookbookGuess::Unknown`] books), and (for confirmed cookbooks) its cover
/// image bytes for the library-grid thumbnail.
struct ScannedBook {
    meta: BookMeta,
    guess: CookbookGuess,
    is_cookbook: bool,
    cover: Option<Vec<u8>>,
    /// Lowercased "title authors" haystack, precomputed at scan time so the
    /// library filter doesn't rebuild it per book per frame.
    search_key: String,
}

/// The `bytes://` URI a library cover is registered under (keyed by book path so
/// each book's thumbnail is distinct).
fn cover_uri(path: &Path) -> String {
    format!("bytes://cover/{}", path.to_string_lossy())
}

type ScanResult = Result<Vec<ScannedBook>, String>;

/// Live extraction progress, shared between the worker thread (which writes it as
/// each chunk finishes) and the UI thread (which reads it each frame to draw the
/// progress bar). Lock-free atomics — Relaxed is fine for a display counter.
#[derive(Default)]
struct ExtractProgressCell {
    done: AtomicUsize,
    total: AtomicUsize,
    cached: AtomicUsize,
}

/// What the library browser wants the caller to do after a frame (kept separate
/// from rendering so the per-row click handlers don't need `&mut self`).
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
type RefGraph =
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

/// State for the Cookbook (EPUB) tab.
pub struct CookbookTab {
    // `pub(crate)` fields are snapshotted by `crate::persist::PersistedState`
    // (inputs and view toggles only — promises and loaded data stay per-run).
    pub(crate) path: String,
    no_cache: bool,
    /// `None` until a load is started.
    promise: Option<Promise<LoadResult>>,
    /// Live chunk-extraction progress for the in-flight `promise`, drawn as a
    /// determinate bar while it runs. Re-created on each load.
    extract_progress: Option<Arc<ExtractProgressCell>>,
    selected: usize,
    /// Whether the central panel shows the reference digraph vs the browser.
    show_graph: bool,
    /// The reference digraph, rebuilt only when the loaded book changes.
    /// `graph_nodes[node_index] = recipe index` for click-to-select.
    graph: Option<RefGraph>,
    /// Whether the layout has been pre-warmed (run headless) for the current
    /// graph, so it opens settled rather than visibly animating into place.
    graph_prewarmed: bool,
    /// The library directory last scanned (for `Re-scan`); also the starting
    /// directory for the file pickers when restored from a previous session.
    pub(crate) library_dir: Option<PathBuf>,
    /// `None` until a library scan is started.
    scan: Option<Promise<ScanResult>>,
    /// Show only books judged to be cookbooks in the library list.
    pub(crate) cookbooks_only: bool,
    /// Run the AI fallback over untagged books during a scan.
    pub(crate) use_ai_fallback: bool,
    /// Free-text filter over the library list (title + authors).
    filter_text: String,
    /// Whether the library browser shows the cover grid (vs. a compact text list).
    pub(crate) library_grid: bool,
    /// Whether the loaded book's image bytes (cover + heroes) have been registered
    /// with egui this load. Reset on each [`Self::start_load`].
    images_registered: bool,
    /// Whether the scanned library's cover bytes have been registered with egui.
    /// Reset on each [`Self::start_scan`].
    library_covers_registered: bool,
}

impl Default for CookbookTab {
    fn default() -> Self {
        Self {
            path: String::new(),
            no_cache: false,
            promise: None,
            extract_progress: None,
            selected: 0,
            show_graph: false,
            graph: None,
            graph_prewarmed: false,
            library_dir: None,
            scan: None,
            // Default the library list to cookbooks only — the whole point of
            // pointing at a Calibre root is to skip the novels.
            cookbooks_only: true,
            use_ai_fallback: false,
            filter_text: String::new(),
            library_grid: true,
            images_registered: false,
            library_covers_registered: false,
        }
    }
}

impl CookbookTab {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        // Source controls: pick a single EPUB, pick a whole library, or type a path.
        ui.horizontal(|ui| {
            if ui.button("📂 Pick EPUB…").clicked() {
                if let Some(p) = self.file_dialog().add_filter("EPUB", &["epub"]).pick_file() {
                    self.path = p.to_string_lossy().into_owned();
                    self.start_load(ctx.clone());
                }
            }
            if ui
                .button("🗂 Pick library…")
                .on_hover_text("Pick a Calibre root (or any folder); finds every .epub inside")
                .clicked()
            {
                if let Some(dir) = self.file_dialog().pick_folder() {
                    self.start_scan(dir, ctx.clone());
                }
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.path)
                    .hint_text("/path/to/cookbook.epub")
                    .desired_width(340.0),
            );
            ui.checkbox(&mut self.no_cache, "No cache");
            let can_load = !self.path.trim().is_empty();
            if ui
                .add_enabled(can_load, egui::Button::new("Load"))
                .clicked()
            {
                self.start_load(ctx.clone());
            }
        });
        ui.label(
            RichText::new(
                "Default model gemini-2.5-flash via the Cloudflare AI Gateway \
                 (reads AI_GATEWAY_API_KEY + CLOUDFLARE_AI_GATEWAY_BASE_URL). First \
                 load runs LLM extraction (cached afterwards).",
            )
            .weak()
            .small(),
        );
        ui.separator();

        // Register image bytes with egui once they're ready (cover thumbnails for
        // the scanned library, cover + hero photos for the loaded book), so the
        // `bytes://…` URIs the views reference resolve.
        self.register_library_covers(&ctx);
        self.register_loaded_images(&ctx);

        // Library browser as a resizable LEFT sidebar — shown only once a library
        // has been scanned. The loaded book (below) fills the area to its right.
        let mut action = LibraryAction::None;
        if self.scan.is_some() {
            egui::Panel::left("library_browser")
                .resizable(true)
                .default_size(320.0)
                .show_inside(ui, |ui| {
                    if let Some(scan) = &self.scan {
                        // Disjoint field borrows: `scan` (shared) vs the
                        // filter/toggle fields (mutable) — distinct `self` fields.
                        let cookbooks_only = &mut self.cookbooks_only;
                        let use_ai = &mut self.use_ai_fallback;
                        let filter = &mut self.filter_text;
                        let grid = &mut self.library_grid;
                        match scan.ready() {
                            None => {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label("Scanning library…");
                                });
                            }
                            Some(Err(err)) => {
                                ui.colored_label(ui.visuals().error_fg_color, err);
                            }
                            Some(Ok(books)) => {
                                action =
                                    show_library(ui, books, cookbooks_only, use_ai, filter, grid);
                            }
                        }
                    }
                });
            match action {
                LibraryAction::None => {}
                LibraryAction::Rescan => {
                    if let Some(dir) = self.library_dir.clone() {
                        self.start_scan(dir, ctx.clone());
                    }
                }
                LibraryAction::Load(path) => {
                    self.path = path;
                    self.start_load(ctx.clone());
                }
            }
        }

        let Some(promise) = &self.promise else {
            ui.label("Pick or load an EPUB to view its recipes.");
            return;
        };

        // Bound before the match so the pending arm can read it alongside the
        // shared `&self.promise` borrow (distinct fields → disjoint borrows).
        let progress = self.extract_progress.as_ref();
        match promise.ready() {
            None => {
                // Once the chunk count is known, show a determinate bar that fills
                // chunk-by-chunk; until then, fall back to the spinner.
                let total = progress.map_or(0, |p| p.total.load(Ordering::Relaxed));
                if total > 0 {
                    let done = progress.map_or(0, |p| p.done.load(Ordering::Relaxed));
                    let cached = progress.map_or(0, |p| p.cached.load(Ordering::Relaxed));
                    ui.add(
                        egui::ProgressBar::new(done as f32 / total as f32).text(format!(
                            "Extracting… {done}/{total} chunks · {cached} cached"
                        )),
                    );
                } else {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Extracting recipes…");
                    });
                }
            }
            Some(Err(err)) => {
                ui.colored_label(ui.visuals().error_fg_color, err);
            }
            Some(Ok(book)) if book.recipes.is_empty() => {
                ui.label("No recipes found in this EPUB.");
            }
            Some(Ok(book)) => {
                let LoadedBook {
                    recipes,
                    stats,
                    cover,
                    images: _,
                } = book;
                // Disjoint field borrows: `recipes` from self.promise (shared);
                // the rest from distinct `self` fields. Bind each before any
                // closure so no closure captures all of `self`.
                let selected = &mut self.selected;
                let show_graph = &mut self.show_graph;
                let graph_slot = &mut self.graph;
                let graph_prewarmed = &mut self.graph_prewarmed;
                if *selected >= recipes.len() {
                    *selected = 0;
                }
                let ref_count: usize = recipes.iter().map(|r| r.references.len()).sum();

                ui.horizontal(|ui| {
                    // The book's cover as a small thumbnail at the head of the row.
                    if let Some(c) = cover {
                        ui.add(
                            egui::Image::from_uri(format!("bytes://{}", c.path))
                                .fit_to_exact_size(egui::vec2(28.0, 38.0))
                                .corner_radius(3.0),
                        );
                    }
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
                    ui.separator();
                    // Copy the whole book's extracted recipes (verbatim lines +
                    // references) as pretty JSON to the clipboard — the same
                    // shape as `scrape-epub --json`.
                    if ui
                        .button("Copy JSON")
                        .on_hover_text("Copy all recipes as JSON to the clipboard")
                        .clicked()
                    {
                        match serde_json::to_string_pretty(recipes) {
                            Ok(json) => ui.ctx().copy_text(json),
                            Err(e) => tracing::error!("copy JSON failed: {e}"),
                        }
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
                            *graph_prewarmed = false;
                        }
                        if let Some(graph) = graph_slot {
                            if let Some(open_idx) =
                                show_reference_graph(ui, graph, selected, graph_prewarmed)
                            {
                                // "Open" clicked on a node: jump to that recipe
                                // in the Browse view.
                                *selected = open_idx;
                                *show_graph = false;
                            }
                        }
                    } else if let Some(nav) = show_recipe_detail(ui, recipes, *selected) {
                        // Clicked a "Uses recipes" link → navigate to it.
                        *selected = nav;
                    }
                });
            }
        }
    }

    /// Register the loaded book's image bytes (cover + every hero) with egui the
    /// first frame the load is ready, so `bytes://<path>` URIs resolve. Idempotent
    /// per load (guarded by `images_registered`).
    fn register_loaded_images(&mut self, ctx: &egui::Context) {
        if self.images_registered {
            return;
        }
        let Some(Ok(book)) = self.promise.as_ref().and_then(Promise::ready) else {
            return;
        };
        for (path, bytes) in &book.images {
            ctx.include_bytes(format!("bytes://{path}"), bytes.clone());
        }
        self.images_registered = true;
    }

    /// Register the scanned library's cover bytes with egui once the scan is ready,
    /// so the grid's `bytes://cover/<path>` thumbnails resolve. Idempotent per scan.
    fn register_library_covers(&mut self, ctx: &egui::Context) {
        if self.library_covers_registered {
            return;
        }
        let Some(Ok(books)) = self.scan.as_ref().and_then(Promise::ready) else {
            return;
        };
        for b in books {
            if let Some(bytes) = &b.cover {
                ctx.include_bytes(cover_uri(&b.meta.path), bytes.clone());
            }
        }
        self.library_covers_registered = true;
    }

    /// A native file dialog starting in the last-scanned library directory
    /// (restored across sessions), so repeat picks don't start from $HOME.
    fn file_dialog(&self) -> rfd::FileDialog {
        let dialog = rfd::FileDialog::new();
        match &self.library_dir {
            Some(dir) => dialog.set_directory(dir),
            None => dialog,
        }
    }

    fn start_load(&mut self, ctx: egui::Context) {
        let path = self.path.trim().to_string();
        let no_cache = self.no_cache;
        self.selected = 0;
        self.graph = None; // rebuilt for the newly loaded book
        self.graph_prewarmed = false;
        self.images_registered = false; // re-register for the newly loaded book

        // Fresh progress cell for this load (so no stale bar from a prior run);
        // one clone stays on `self` for the UI, the other goes to the worker.
        let progress = Arc::new(ExtractProgressCell::default());
        self.extract_progress = Some(progress.clone());
        let progress_ctx = ctx.clone();
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
                let (recipes, stats) = rt
                    .block_on(recipe_epub::extract_cookbook_with_progress(
                        &bytes,
                        &path,
                        &opts,
                        |p| {
                            // Publish the latest counts and wake the UI so the bar
                            // advances live as each chunk lands.
                            progress.done.store(p.done, Ordering::Relaxed);
                            progress.total.store(p.total, Ordering::Relaxed);
                            progress.cached.store(p.cached, Ordering::Relaxed);
                            progress_ctx.request_repaint();
                        },
                    ))
                    .map_err(|e| e.to_string())?;
                // Materialize the cover + each recipe's hero photo in one EPUB open,
                // off the UI thread, so the views just reference the registered bytes.
                let (cover, images) = recipe_epub::collect_recipe_images(&bytes, &recipes);
                Ok(LoadedBook {
                    recipes,
                    stats,
                    cover,
                    images,
                })
            })();
            // Wake the UI thread when extraction finishes (poll-promise doesn't
            // repaint on its own).
            ctx.request_repaint();
            result
        }));
    }

    /// Scan a directory for epubs and classify each as cookbook-or-not. Runs off
    /// the UI thread (like [`Self::start_load`]); reading each book's OPF is cheap
    /// (lazy, no content decompression). When `use_ai_fallback` is on, the
    /// untagged ([`CookbookGuess::Unknown`]) books are settled by one batched LLM
    /// call.
    fn start_scan(&mut self, dir: PathBuf, ctx: egui::Context) {
        self.library_dir = Some(dir.clone());
        self.library_covers_registered = false; // re-register for the new scan
        let use_ai = self.use_ai_fallback;
        self.scan = Some(Promise::spawn_thread("scan_library", move || {
            let result = (|| -> ScanResult {
                let mut books: Vec<ScannedBook> = recipe_epub::find_epubs(&dir)
                    .iter()
                    .filter_map(|p| recipe_epub::book_metadata(p).ok())
                    .map(|meta| {
                        let guess = recipe_epub::classify_by_tags(&meta);
                        let search_key =
                            format!("{} {}", meta.title, meta.authors.join(" ")).to_lowercase();
                        ScannedBook {
                            is_cookbook: guess == CookbookGuess::Yes,
                            guess,
                            meta,
                            cover: None,
                            search_key,
                        }
                    })
                    .collect();

                // AI fallback: classify only the untagged books the heuristic
                // couldn't settle, in one batched call.
                if use_ai {
                    let unknown: Vec<usize> = books
                        .iter()
                        .enumerate()
                        .filter(|(_, b)| b.guess == CookbookGuess::Unknown)
                        .map(|(i, _)| i)
                        .collect();
                    if !unknown.is_empty() {
                        let metas: Vec<BookMeta> =
                            unknown.iter().map(|&i| books[i].meta.clone()).collect();
                        let rt = tokio::runtime::Builder::new_multi_thread()
                            .enable_all()
                            .build()
                            .map_err(|e| format!("tokio runtime: {e}"))?;
                        let verdicts = rt
                            .block_on(recipe_epub::classify_cookbooks_ai(
                                &metas,
                                &Options::default(),
                            ))
                            .map_err(|e| e.to_string())?;
                        for (&i, is_cookbook) in unknown.iter().zip(verdicts) {
                            books[i].is_cookbook = is_cookbook;
                        }
                    }
                }

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
            })();
            ctx.request_repaint();
            result
        }));
    }
}

/// Render the scanned-library browser: a summary line, the cookbooks-only / AI
/// toggles + re-scan, a search box, and the (filtered) book list. Returns the
/// action the caller should take with `&mut self` (a row click → load, the
/// button → re-scan).
fn show_library(
    ui: &mut egui::Ui,
    books: &[ScannedBook],
    cookbooks_only: &mut bool,
    use_ai: &mut bool,
    filter: &mut String,
    grid: &mut bool,
) -> LibraryAction {
    let mut action = LibraryAction::None;
    let cookbook_count = books.iter().filter(|b| b.is_cookbook).count();
    let unknown_count = books
        .iter()
        .filter(|b| b.guess == CookbookGuess::Unknown)
        .count();

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
        ui.checkbox(use_ai, "Use AI for untagged")
            .on_hover_text(format!(
                "{unknown_count} book(s) have no tags to judge from. Enable, then Re-scan to \
             classify them with the model.",
            ));
        // Toggle the cover grid off to reclaim space (falls back to a compact list).
        ui.toggle_value(grid, "🖼 Covers")
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
        ui.label("🔎");
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
                        action = LibraryAction::Load(b.meta.path.to_string_lossy().into_owned());
                    }
                }
            }
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
                    "📕",
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

/// Render the selected recipe's detail view. Returns `Some(idx)` if the user
/// clicked one of the "Uses recipes" links, so the caller can navigate there.
fn show_recipe_detail(
    ui: &mut egui::Ui,
    recipes: &[CookbookRecipe],
    selected: usize,
) -> Option<usize> {
    let r = &recipes[selected];
    let mut navigate_to = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        // The recipe's hero photo, if one was found near its title (bytes were
        // registered under `bytes://<path>` when the book loaded).
        if let Some(img) = &r.image {
            ui.add(
                egui::Image::from_uri(format!("bytes://{}", img.path))
                    .max_height(240.0)
                    .max_width(ui.available_width())
                    .corner_radius(6.0),
            );
            ui.add_space(6.0);
        }
        ui.heading(&r.meta.title);
        if let Some(category) = &r.meta.category {
            ui.label(RichText::new(category).italics().weak());
        }

        ui.horizontal_wrapped(|ui| {
            if let Some(y) = &r.meta.recipe_yield {
                ui.label(RichText::new(format!("{} {y}", theme::icon::YIELD)).color(theme::AMOUNT));
            }
            if let Some(t) = &r.meta.times {
                for (label, value) in [
                    ("active", &t.active),
                    ("total", &t.total),
                    ("prep", &t.prep),
                    ("cook", &t.cook),
                ] {
                    if let Some(value) = value {
                        ui.label(format!("{} {label}: {value}", theme::icon::TIME));
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
        // color-coded view (cheap — one recipe per frame). Ingredient lines that
        // reference another recipe get a clickable "→ <recipe>" link.
        let parsed = r.parse();
        if let Some(nav) = show_sections_with_links(ui, &r.sections, &parsed.sections, r, recipes) {
            navigate_to = Some(nav);
        }

        if !r.meta.equipment.is_empty() || !r.meta.notes.is_empty() {
            ui.separator();
            for e in &r.meta.equipment {
                ui.label(format!("{} {e}", theme::icon::EQUIPMENT));
            }
            for n in &r.meta.notes {
                ui.label(RichText::new(format!("{} {n}", theme::icon::NOTE)).weak());
            }
        }

        // Other recipes in this book that this one uses as ingredients —
        // rendered as links that navigate to the referenced recipe.
        if !r.references.is_empty() {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("↳ Uses recipes:").strong());
                for reference in &r.references {
                    // Resolve the reference title to a recipe index (titles are
                    // unique per book). If found, render a clickable link.
                    let target = recipes.iter().position(|o| o.meta.title == reference.title);
                    if let Some(idx) = target {
                        if ui.link(&reference.title).clicked() {
                            navigate_to = Some(idx);
                        }
                    } else {
                        ui.label(&reference.title);
                    }
                }
            });
        }

        // The reverse edge: other recipes in this book that reference THIS one
        // (e.g. "Brioche Dough" is used by the Apricot Tart, the Bostock, …).
        let this_title = r.meta.title.as_str();
        let used_by: Vec<usize> = recipes
            .iter()
            .enumerate()
            .filter(|(i, o)| *i != selected && o.references.iter().any(|x| x.title == this_title))
            .map(|(i, _)| i)
            .collect();
        if !used_by.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("↰ Used by:").strong());
                for idx in used_by {
                    if ui.link(&recipes[idx].meta.title).clicked() {
                        navigate_to = Some(idx);
                    }
                }
            });
        }

        ui.add_space(8.0);
        ui.label(RichText::new(&r.url).weak().small());
    });
    navigate_to
}

/// Render a cookbook recipe's parsed sections (color-coded ingredients +
/// measurement-aware instructions), like `recipe::show_parsed_sections`, but
/// with an extra clickable "→ <recipe>" link after any ingredient line that
/// references another recipe. Returns the recipe index to navigate to if a link
/// was clicked.
///
/// `raw` is the verbatim section list (whose ingredient strings are matched
/// against `recipe.references[].line`); `parsed` is the same sections after
/// parsing. They align 1:1.
fn show_sections_with_links(
    ui: &mut egui::Ui,
    raw: &[recipe_epub::RecipeSection],
    parsed: &[recipe_epub::ParsedSection],
    recipe: &CookbookRecipe,
    recipes: &[CookbookRecipe],
) -> Option<usize> {
    let mut navigate_to = None;
    for (raw_sec, sec) in raw.iter().zip(parsed) {
        if let Some(name) = &sec.name {
            ui.heading(name);
        }
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                for (raw_line, ing) in raw_sec.ingredients.iter().zip(&sec.ingredients) {
                    theme::card_compact(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            super::recipe::show_ingredient_collapsing(ui, ing);
                            // If this verbatim line is a cross-recipe reference, add
                            // a link to the target recipe.
                            if let Some(idx) = recipe
                                .references
                                .iter()
                                .find(|x| &x.line == raw_line)
                                .and_then(|x| recipes.iter().position(|o| o.meta.title == x.title))
                            {
                                if ui
                                    .link(
                                        RichText::new(format!("{} open", theme::icon::OPEN))
                                            .small(),
                                    )
                                    .clicked()
                                {
                                    navigate_to = Some(idx);
                                }
                            }
                        });
                    });
                }
            });
            ui.vertical(|ui| {
                for instr in &sec.instructions {
                    super::recipe::show_instruction_chunks(ui, instr);
                }
            });
        });
    }
    navigate_to
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
                node.set_color(theme::GRAPH_NODE);
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
fn show_reference_graph(
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
    if let Some(nid) = graph.selected_nodes().first().copied() {
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
        epaint::{CircleShape, TextShape},
        Color32, FontFamily, FontId, Pos2, Shape, Stroke, Vec2,
    };
    use egui_graphs::{DisplayNode, DrawContext, NodeProps};
    use petgraph::{stable_graph::IndexType, EdgeType};

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
            let mut res = vec![CircleShape {
                center,
                radius,
                fill: color,
                stroke: Stroke::default(),
            }
            .into()];

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
