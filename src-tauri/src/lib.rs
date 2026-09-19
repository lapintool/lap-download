mod download;
mod json_util;
mod locale;
mod paths;
mod settings;
mod window_state;

use download::{TaskManager, TaskRecord};
use locale::{LocaleFile, LocaleInfo};
use settings::{Settings, SettingsPatch};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
fn uses_custom_titlebar() -> bool {
    cfg!(windows)
}

#[tauri::command]
fn save_window_state_cmd(app: AppHandle) -> Result<(), String> {
    window_state::save_window_state(&app)
}

#[tauri::command]
fn get_settings() -> Settings {
    settings::load_settings()
}

#[tauri::command]
fn update_settings(patch: SettingsPatch) -> Result<Settings, String> {
    settings::update_settings(patch)
}

#[tauri::command]
fn resolve_save_dir() -> String {
    paths::to_display_path(&settings::load_settings().resolve_save_dir())
}

#[tauri::command]
async fn list_tasks(mgr: State<'_, Arc<TaskManager>>) -> Result<Vec<TaskRecord>, String> {
    Ok(mgr.list().await)
}

#[tauri::command]
async fn add_urls(
    app: AppHandle,
    mgr: State<'_, Arc<TaskManager>>,
    urls: Vec<String>,
) -> Result<Vec<TaskRecord>, String> {
    let added = mgr.add_urls(urls).await?;
    let all = mgr.list().await;
    let _ = app.emit("download-updated", all);
    Ok(added)
}

#[tauri::command]
async fn start_downloads(app: AppHandle, mgr: State<'_, Arc<TaskManager>>) -> Result<(), String> {
    mgr.start_queue(app);
    Ok(())
}

#[tauri::command]
async fn cancel_task(
    app: AppHandle,
    mgr: State<'_, Arc<TaskManager>>,
    id: String,
) -> Result<(), String> {
    mgr.cancel(&id).await?;
    let all = mgr.list().await;
    let _ = app.emit("download-updated", all);
    Ok(())
}

#[tauri::command]
async fn remove_task(
    app: AppHandle,
    mgr: State<'_, Arc<TaskManager>>,
    id: String,
) -> Result<(), String> {
    mgr.remove(&id).await?;
    let all = mgr.list().await;
    let _ = app.emit("download-updated", all);
    Ok(())
}

#[tauri::command]
async fn clear_finished(app: AppHandle, mgr: State<'_, Arc<TaskManager>>) -> Result<(), String> {
    mgr.clear_finished().await?;
    let all = mgr.list().await;
    let _ = app.emit("download-updated", all);
    Ok(())
}

#[tauri::command]
async fn pick_save_dir(app: AppHandle, title: Option<String>) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let title = title.unwrap_or_else(|| "选择下载目录".into());
    let folder = app
        .dialog()
        .file()
        .set_title(&title)
        .blocking_pick_folder();
    Ok(folder
        .and_then(|p| p.into_path().ok())
        .map(|p| paths::to_display_path(&p)))
}

#[tauri::command]
fn open_path(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| format!("open path: {e}"))
}

#[tauri::command]
fn list_ui_locales(app: AppHandle) -> Result<Vec<LocaleInfo>, String> {
    locale::list_locales(&app)
}

#[tauri::command]
fn load_ui_locale(app: AppHandle, id: String) -> Result<LocaleFile, String> {
    locale::load_locale(&app, &id)
}

#[tauri::command]
fn set_zoom(window: tauri::WebviewWindow, level: f64) -> Result<f64, String> {
    let zoom = level.clamp(settings::ZOOM_MIN, settings::ZOOM_MAX);
    window.set_zoom(zoom).map_err(|e| e.to_string())?;
    let mut s = settings::load_settings();
    s.zoom = zoom;
    settings::save_settings(&s)?;
    Ok(zoom)
}

#[tauri::command]
fn zoom_by(window: tauri::WebviewWindow, delta: f64) -> Result<f64, String> {
    let current = settings::load_settings().zoom;
    set_zoom(window, current + delta)
}

fn focus_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = paths::ensure_layout();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            focus_main_window(app);
        }))
        .manage(Arc::new(TaskManager::new()))
        .invoke_handler(tauri::generate_handler![
            uses_custom_titlebar,
            save_window_state_cmd,
            get_settings,
            update_settings,
            resolve_save_dir,
            list_tasks,
            add_urls,
            start_downloads,
            cancel_task,
            remove_task,
            clear_finished,
            pick_save_dir,
            open_path,
            list_ui_locales,
            load_ui_locale,
            set_zoom,
            zoom_by,
        ])
        .setup(|app| {
            paths::seed_locale_files(app.handle());
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(icon) =
                    tauri::image::Image::from_bytes(include_bytes!("../icons/128x128.png"))
                {
                    let _ = window.set_icon(icon);
                }
                let _ = window.set_title(&format!("lapdw {}", app.package_info().version));
                #[cfg(windows)]
                {
                    let _ = window.set_decorations(false);
                    let _ = window.set_shadow(true);
                }
                window_state::restore_window_state(&window);
                let loaded = settings::load_settings();
                let _ = window.set_zoom(loaded.zoom);
                let handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                        let _ = window_state::save_window_state(&handle);
                    }
                });
                let _ = window.show();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running lapdw");
}
