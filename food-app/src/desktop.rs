//! Native shell only; application execution lives in the portable backend.
use crate::{backend as service, operations::Operations};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager, State, ipc::Channel};

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
async fn extract_run(
    request: service::ExtractionRequest,
    on_progress: Channel<service::ExtractionProgress>,
    operations: State<'_, Operations>,
) -> Result<service::CookbookResult, String> {
    let mut paths = vec![request.out.as_str()];
    if let Some(parent) = &request.from {
        paths.push(parent);
    }
    let _guard = operations.acquire(&paths)?;
    service::extract_run(request, move |progress| {
        let _ = on_progress.send(progress);
    })
    .await
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
            extract_run
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
