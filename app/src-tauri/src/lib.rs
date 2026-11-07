//! The Tauri shell around `esmm-core` (decision K in CLAUDE.md).
//!
//! - [`shell`]: the Tauri-independent service holding in-memory state and doing all the work.
//! - [`commands`]: the `#[tauri::command]` wrappers the frontend invokes.
//! - [`views`] / [`error`]: the serializable contract, exported to `app/src/bindings/`.

mod commands;
mod error;
mod fetcher;
mod logging;
mod overlaps;
mod settings;
mod shell;
mod views;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use tauri::{Emitter, Manager, Runtime};

use crate::commands::AppState;
use crate::fetcher::ProgressSink;
use crate::shell::{Deps, Shell};

/// The event carrying [`views::DownloadProgress`] while a plan downloads.
pub const DOWNLOAD_PROGRESS_EVENT: &str = "download-progress";

/// Registers every command. Shared by [`run`] and the IPC tests so they can't diverge.
fn with_commands<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.invoke_handler(tauri::generate_handler![
        commands::load_catalog,
        commands::get_icon,
        commands::list_installs,
        commands::select_install,
        commands::add_custom_install,
        commands::remove_custom_install,
        commands::game_status,
        commands::launch_game,
        commands::get_state,
        commands::adopt_plugin,
        commands::plan_install,
        commands::plan_update,
        commands::plan_update_all,
        commands::plan_enable,
        commands::plan_disable,
        commands::plan_uninstall,
        commands::plan_apply_profile,
        commands::cancel_planning,
        commands::resolve_conflict,
        commands::discard_plan,
        commands::commit_plan,
        commands::create_profile,
        commands::create_empty_profile,
        commands::rename_profile,
        commands::export_profile,
        commands::import_profile,
        commands::update_active_profile,
        commands::delete_profile,
        commands::get_overlap_scan,
        commands::set_overlap_scan,
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    // Must be the first plugin registered. A second launch (a double click, or a desktop
    // launcher firing twice) hands over to the copy that is already running, which brings its
    // window to the front, and then exits instead of opening another window.
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }));
    let builder = builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            if let Ok(log_dir) = app.path().app_log_dir()
                && let Some(guard) = logging::init(&log_dir)
            {
                // The process id, the program's path and (for an AppImage) the AppImage file
                // are logged so a report of "it opens many copies" can be told apart from a
                // crash-and-relaunch loop or a launcher that fires twice.
                tracing::info!(
                    version = env!("CARGO_PKG_VERSION"),
                    pid = std::process::id(),
                    exe = ?std::env::current_exe().ok(),
                    appimage = ?std::env::var_os("APPIMAGE"),
                    "esmm starting"
                );
                // Kept alive for the app's lifetime via Tauri's state management;
                // dropping it would stop the background writer thread.
                app.manage(guard);
            }
            let handle = app.handle().clone();
            let progress: ProgressSink = Arc::new(move |p| {
                let _ = handle.emit(DOWNLOAD_PROGRESS_EVENT, p);
            });
            let deps = Deps::production(
                app.path().app_data_dir()?,
                app.path().app_cache_dir()?,
                progress,
            );
            app.manage(AppState::new(Shell::new(deps)));
            Ok(())
        });
    with_commands(builder)
        .run(tauri::generate_context!())
        .expect("error while running the Endless Sky Mod Manager");
}
