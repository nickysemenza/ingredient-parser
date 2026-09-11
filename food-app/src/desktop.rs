//! Native shell only; application execution lives in the portable backend.
use crate::backend as service;
use cookbook::CancelToken;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager, State, ipc::Channel};
use tauri_plugin_opener::OpenerExt;

#[derive(Default)]
struct CloseState(AtomicBool);

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
) -> Result<(), String> {
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
async fn reveal_file(app: tauri::AppHandle, path: String) -> Result<(), String> {
    blocking(move || {
        let resolved = std::path::Path::new(&path)
            .canonicalize()
            .map_err(|e| format!("Cannot find {path}. Locate the file and open it again: {e}"))?;
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
async fn open_book(path: String) -> Result<service::OpenedBook, String> {
    blocking(move || service::open_book(path)).await
}
#[tauri::command]
async fn estimate_book(path: String) -> Result<cookbook::Estimate, String> {
    blocking(move || service::estimate_book(path)).await
}
#[tauri::command]
fn gateway_status() -> service::GatewayStatus {
    service::gateway_status()
}
#[tauri::command]
async fn list_runs() -> Result<Vec<service::RunSummary>, String> {
    blocking(service::list_runs).await
}
#[tauri::command]
async fn open_run(path: String) -> Result<cookbook::Extraction, String> {
    blocking(move || service::open_run(path)).await
}
#[tauri::command]
async fn delete_run(path: String) -> Result<(), String> {
    blocking(move || service::delete_run(path)).await
}
#[tauri::command]
async fn book_image(book: String, image: String) -> Result<service::BookImage, String> {
    blocking(move || service::book_image(book, image)).await
}
#[tauri::command]
async fn load_cover(path: String) -> Result<Option<service::BookImage>, String> {
    blocking(move || service::load_cover(path)).await
}

/// One extraction at a time; the token cancels it.
#[derive(Default)]
struct ExtractionState(std::sync::Mutex<Option<CancelToken>>);

#[tauri::command]
fn cancel_extraction(state: State<'_, ExtractionState>) -> Result<(), String> {
    if let Some(cancel) = state
        .0
        .lock()
        .map_err(|_| "Extraction state unavailable")?
        .as_ref()
    {
        cancel.cancel();
    }
    Ok(())
}

#[tauri::command]
async fn extract_book(
    path: String,
    on_progress: Channel<cookbook::Progress>,
    state: State<'_, ExtractionState>,
) -> Result<service::RunSummary, String> {
    let cancel = CancelToken::new();
    {
        let mut active = state.0.lock().map_err(|_| "Extraction state unavailable")?;
        if active.is_some() {
            return Err("An extraction is already running".into());
        }
        *active = Some(cancel.clone());
    }
    // The run drives many borrowed per-chunk futures, which rustc cannot
    // prove `Send` across an executor boundary; a dedicated runtime on a
    // blocking thread keeps them on one thread. Everything captured is owned.
    let worker = cancel.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("Could not start the extraction runtime: {error}"))?;
        runtime.block_on(service::extract_book(path, worker, move |progress| {
            let _ = on_progress.send(progress);
        }))
    })
    .await
    .map_err(|error| format!("The extraction stopped unexpectedly: {error}"))
    .and_then(|r| r);
    *state.0.lock().map_err(|_| "Extraction state unavailable")? = None;
    result
}

pub fn run() -> tauri::Result<()> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
    let app = tauri::Builder::default()
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
            let window_menu = SubmenuBuilder::new(app, "Window")
                .minimize()
                .maximize()
                .build()?;
            app.set_menu(
                MenuBuilder::new(app)
                    .items(&[&app_menu, &file_menu, &edit_menu, &view_menu, &window_menu])
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
            set_close_blocked,
            quit_app,
            parse_batch,
            inspect_ingredient,
            load_recipe,
            scale_web_recipe,
            load_corpus,
            scan_library,
            open_book,
            estimate_book,
            gateway_status,
            list_runs,
            open_run,
            delete_run,
            book_image,
            load_cover,
            extract_book,
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
