//! Native shell only; application execution lives in the portable backend.
use crate::{backend as service, operations::Operations};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager, State, ipc::Channel};
use tauri_plugin_opener::OpenerExt;

#[derive(Default)]
struct CloseState(AtomicBool);

#[tauri::command]
fn startup_run() -> Option<String> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--review-run" {
            return args.next().map(|path| path.to_string_lossy().into_owned());
        }
    }
    None
}

#[tauri::command]
fn set_close_blocked(blocked: bool, state: State<'_, CloseState>) {
    state.0.store(blocked, Ordering::Relaxed);
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle, state: State<'_, CloseState>) {
    state.0.store(false, Ordering::Relaxed);
    app.exit(0);
}

#[tauri::command]
fn set_shell_appearance(
    window: tauri::WebviewWindow,
    dark: bool,
    title: String,
    review_enabled: bool,
) -> Result<(), String> {
    if let Some(menu) = window.app_handle().menu()
        && let Some(item) = menu.get("review-menu")
        && let Some(submenu) = item.as_submenu()
    {
        for id in [
            "review-accept",
            "review-incorrect",
            "review-uncertain",
            "review-next",
        ] {
            if let Some(item) = submenu.get(id)
                && let Some(item) = item.as_menuitem()
            {
                item.set_enabled(review_enabled)
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    window.set_title(&title).map_err(|e| e.to_string())?;
    window
        .set_theme(Some(if dark {
            tauri::Theme::Dark
        } else {
            tauri::Theme::Light
        }))
        .map_err(|e| e.to_string())?;
    let color = if dark {
        tauri::window::Color(30, 31, 34, 255)
    } else {
        tauri::window::Color(249, 249, 251, 255)
    };
    window
        .set_background_color(Some(color))
        .map_err(|e| e.to_string())
}

fn source_url(value: &str) -> Result<tauri::Url, String> {
    let url = tauri::Url::parse(value).map_err(|_| "The source URL is invalid.".to_owned())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("Only HTTP and HTTPS recipe URLs can be opened.".into());
    }
    Ok(url)
}

#[tauri::command]
async fn open_source_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let url = source_url(&url)?;
    blocking(move || {
        app.opener()
            .open_url(url.as_str(), None::<&str>)
            .map_err(|e| format!("Could not open the recipe in your browser: {e}"))
    })
    .await
}

#[tauri::command]
async fn reveal_file(app: tauri::AppHandle, path: String, review: bool) -> Result<(), String> {
    blocking(move || {
        let path = if review {
            service::review_path(&path)?
        } else {
            path
        };
        let resolved = std::path::Path::new(&path).canonicalize().map_err(|e| {
            if review {
                format!("The review file is unavailable. Save a review first, then try again: {e}")
            } else {
                format!("Cannot find {path}. Locate the file and open it again: {e}")
            }
        })?;
        app.opener()
            .reveal_item_in_dir(resolved)
            .map_err(|e| format!("Could not reveal the file in Finder: {e}"))
    })
    .await
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| format!("The operation stopped unexpectedly: {error}"))?
}

#[tauri::command]
async fn parse_batch(input: String) -> Result<Vec<service::IngredientResult>, String> {
    blocking(move || service::parse_batch(input)).await
}
#[tauri::command]
async fn inspect_ingredient(input: String) -> Result<service::IngredientInspection, String> {
    blocking(move || service::inspect_ingredient(input)).await
}
#[tauri::command]
async fn load_recipe(url: String) -> Result<service::RecipeResult, String> {
    service::load_recipe(url).await
}
#[tauri::command]
async fn scale_web_recipe(
    source: serde_json::Value,
    factor: f64,
) -> Result<service::RecipeResult, String> {
    blocking(move || service::scale_web_recipe(source, factor)).await
}
#[tauri::command]
async fn load_corpus(path: Option<String>) -> Result<service::CorpusResult, String> {
    blocking(move || service::load_corpus(path)).await
}
#[tauri::command]
async fn scan_library(directory: String) -> Result<Vec<service::LibraryBook>, String> {
    blocking(move || service::scan_library(directory)).await
}
#[tauri::command]
async fn inspect_book(
    path: String,
    model: Option<String>,
) -> Result<service::CookbookResult, String> {
    blocking(move || service::inspect_book(path, model)).await
}
#[tauri::command]
async fn open_run(path: String) -> Result<service::CookbookResult, String> {
    blocking(move || service::open_run(path)).await
}
#[tauri::command]
fn cookbook_models() -> Vec<service::ModelChoice> {
    service::cookbook_models()
}
#[tauri::command]
async fn cookbook_results(book: Option<String>) -> Result<service::ModelBookResults, String> {
    blocking(move || service::cookbook_results(book)).await
}
#[tauri::command]
async fn cookbook_runs(book: Option<String>) -> Result<Vec<service::SavedRun>, String> {
    blocking(move || service::cookbook_runs(book)).await
}
#[tauri::command]
async fn extraction_preview(
    request: service::ExtractionRequest,
) -> Result<service::ExtractionPreview, String> {
    blocking(move || service::extraction_preview(request)).await
}
#[tauri::command]
async fn export_run(path: String, out: String) -> Result<(), String> {
    blocking(move || service::export_run(path, out)).await
}
#[derive(Default)]
struct ExtractionState(std::sync::Mutex<Option<recipe_epub::review::ExtractionControl>>);

#[tauri::command]
fn cancel_extraction(state: State<'_, ExtractionState>) -> Result<(), String> {
    if let Some(control) = state
        .0
        .lock()
        .map_err(|_| "Extraction state unavailable")?
        .as_ref()
    {
        control.cancel();
    }
    Ok(())
}

#[tauri::command]
async fn extract_run(
    request: service::ExtractionRequest,
    on_progress: Channel<service::ExtractionProgress>,
    extraction_state: State<'_, ExtractionState>,
    operations: State<'_, Operations>,
) -> Result<service::CookbookResult, String> {
    let mut paths = if request.out.is_empty() {
        vec![]
    } else {
        vec![request.out.as_str()]
    };
    if let Some(parent) = &request.from {
        paths.push(parent);
    }
    let _guard = operations.acquire(&paths)?;
    let control = recipe_epub::review::ExtractionControl::default();
    {
        let mut active = extraction_state
            .0
            .lock()
            .map_err(|_| "Extraction state unavailable")?;
        if active.is_some() {
            return Err("An extraction is already running".into());
        }
        *active = Some(control.clone());
    }
    let result = service::extract_run_controlled(request, &control, move |progress| {
        let _ = on_progress.send(progress);
    })
    .await;
    *extraction_state
        .0
        .lock()
        .map_err(|_| "Extraction state unavailable")? = None;
    result
}
#[tauri::command]
async fn replay_run(
    path: String,
    out: String,
    operations: State<'_, Operations>,
) -> Result<service::CookbookResult, String> {
    let guard = operations.acquire(&[&path, &out])?;
    blocking(move || {
        let _guard = guard;
        service::replay_run(path, out)
    })
    .await
}
#[tauri::command]
async fn save_review(
    path: String,
    document: String,
    status: String,
    note: String,
    operations: State<'_, Operations>,
) -> Result<service::CookbookResult, String> {
    let guard = operations.acquire(&[&path])?;
    blocking(move || {
        let _guard = guard;
        service::save_review(path, document, status, note)
    })
    .await
}
#[tauri::command]
async fn run_stats(path: String) -> Result<serde_json::Value, String> {
    blocking(move || service::run_stats(path)).await
}
#[tauri::command]
async fn evaluate_run(path: String, expectations: String) -> Result<serde_json::Value, String> {
    blocking(move || service::evaluate_run(path, expectations)).await
}
#[tauri::command]
async fn run_audit(path: String) -> Result<serde_json::Value, String> {
    blocking(move || service::run_audit(path)).await
}
#[tauri::command]
async fn run_diff(before: String, after: String) -> Result<serde_json::Value, String> {
    blocking(move || service::run_diff(before, after)).await
}
#[tauri::command]
async fn scale_recipe(
    path: String,
    index: usize,
    factor: f64,
) -> Result<service::CookbookRecipe, String> {
    blocking(move || service::scale_recipe(path, index, factor)).await
}
#[tauri::command]
async fn load_cover(path: String) -> Result<Option<service::BookImage>, String> {
    blocking(move || service::load_cover(path)).await
}
#[tauri::command]
async fn load_images(
    run_path: Option<String>,
    book_path: String,
) -> Result<Vec<service::BookImage>, String> {
    blocking(move || service::load_images(run_path, book_path)).await
}

pub fn run() -> tauri::Result<()> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
    let app = tauri::Builder::default()
        .manage(Operations::default())
        .manage(CloseState::default())
        .manage(ExtractionState::default())
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(|app| {
            let app_menu = SubmenuBuilder::new(app, "Ingredient Parser")
                .about(None)
                .separator()
                .hide()
                .hide_others()
                .show_all()
                .separator()
                .item(
                    &MenuItemBuilder::with_id("quit", "Quit Ingredient Parser")
                        .accelerator("CmdOrCtrl+Q")
                        .build(app)?,
                )
                .build()?;
            let file_menu = SubmenuBuilder::new(app, "File")
                .item(
                    &MenuItemBuilder::with_id("open", "Open…")
                        .accelerator("CmdOrCtrl+O")
                        .build(app)?,
                )
                .item(
                    &MenuItemBuilder::with_id("save", "Save Review")
                        .accelerator("CmdOrCtrl+S")
                        .build(app)?,
                )
                .close_window()
                .build()?;
            let edit_menu = SubmenuBuilder::new(app, "Edit")
                .undo()
                .redo()
                .separator()
                .cut()
                .copy()
                .paste()
                .select_all()
                .build()?;
            let view_menu = SubmenuBuilder::new(app, "View")
                .item(
                    &MenuItemBuilder::with_id("parser", "Parser")
                        .accelerator("CmdOrCtrl+1")
                        .build(app)?,
                )
                .item(
                    &MenuItemBuilder::with_id("cookbooks", "Cookbooks")
                        .accelerator("CmdOrCtrl+2")
                        .build(app)?,
                )
                .fullscreen()
                .build()?;
            let review_menu = SubmenuBuilder::with_id(app, "review-menu", "Review")
                .item(
                    &MenuItemBuilder::with_id("review-accept", "Mark Accepted")
                        .accelerator("CmdOrCtrl+Alt+A")
                        .build(app)?,
                )
                .item(
                    &MenuItemBuilder::with_id("review-incorrect", "Mark Incorrect")
                        .accelerator("CmdOrCtrl+Alt+I")
                        .build(app)?,
                )
                .item(
                    &MenuItemBuilder::with_id("review-uncertain", "Mark Uncertain")
                        .accelerator("CmdOrCtrl+Alt+U")
                        .build(app)?,
                )
                .separator()
                .item(
                    &MenuItemBuilder::with_id("review-next", "Save and Next Unreviewed")
                        .accelerator("CmdOrCtrl+Shift+Enter")
                        .build(app)?,
                )
                .build()?;
            let window_menu = SubmenuBuilder::new(app, "Window")
                .minimize()
                .maximize()
                .build()?;
            app.set_menu(
                MenuBuilder::new(app)
                    .items(&[
                        &app_menu,
                        &file_menu,
                        &edit_menu,
                        &view_menu,
                        &review_menu,
                        &window_menu,
                    ])
                    .build()?,
            )?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.state::<CloseState>().0.load(Ordering::Relaxed)
            {
                api.prevent_close();
                let _ = window.emit("app-menu", "quit");
            }
        })
        .on_menu_event(|app, event| {
            let _ = app.emit_to("main", "app-menu", event.id().as_ref());
        })
        .invoke_handler(tauri::generate_handler![
            set_shell_appearance,
            reveal_file,
            open_source_url,
            startup_run,
            set_close_blocked,
            quit_app,
            parse_batch,
            inspect_ingredient,
            load_recipe,
            load_corpus,
            scan_library,
            inspect_book,
            open_run,
            replay_run,
            save_review,
            run_stats,
            evaluate_run,
            run_audit,
            run_diff,
            load_images,
            load_cover,
            scale_recipe,
            scale_web_recipe,
            cookbook_models,
            cookbook_runs,
            cookbook_results,
            extraction_preview,
            export_run,
            extract_run,
            cancel_extraction
        ])
        .build(tauri::generate_context!())?;
    app.run(|handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event
            && handle.state::<CloseState>().0.load(Ordering::Relaxed)
        {
            api.prevent_exit();
            let _ = handle.emit_to("main", "app-menu", "quit");
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_recipe_urls_only_allow_web_protocols() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "tauri://localhost",
            "not a URL",
        ] {
            assert!(source_url(url).is_err(), "{url}");
        }
        assert!(source_url("https://example.com/recipe?servings=2").is_ok());
        assert!(source_url("http://localhost/recipe").is_ok());
    }
}
