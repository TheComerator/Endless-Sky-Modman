//! The Tauri shell around `esmm-core` (decision K in CLAUDE.md).
//!
//! - [`shell`]: the Tauri-independent service holding in-memory state and doing all the work.
//! - [`commands`]: the `#[tauri::command]` wrappers the frontend invokes.
//! - [`views`] / [`error`]: the serializable contract, exported to `app/src/bindings/`.

mod commands;
mod error;
mod fetcher;
mod logging;
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
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            if let Ok(log_dir) = app.path().app_log_dir()
                && let Some(guard) = logging::init(&log_dir)
            {
                tracing::info!(version = env!("CARGO_PKG_VERSION"), "esmm starting");
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
