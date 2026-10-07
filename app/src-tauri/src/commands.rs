//! The `#[tauri::command]` surface: thin async wrappers over [`Shell`].
//!
//! Every command returns `Result<_, CmdError>`, so a rejected `invoke` on the frontend always
//! carries a `{ kind, message }` object. Anything that touches the disk, the network or the
//! process list runs off the async runtime (`spawn_blocking`, or a dedicated thread for
//! planning). Argument names arrive camelCase from JavaScript (`planId`, `catalogName`),
//! which is Tauri's default mapping of these snake_case parameters.

use std::sync::{Arc, Mutex};

use tauri::State;
use tokio::sync::oneshot;

use crate::error::{CmdError, CmdResult};
use crate::shell::{Shell, Ticket};
use crate::views::*;

pub struct AppState {
    pub shell: Arc<Shell>,
    /// Wakes the command awaiting the current planning run when it's cancelled.
    abort: Mutex<Option<oneshot::Sender<()>>>,
}

impl AppState {
    pub fn new(shell: Shell) -> Self {
        AppState {
            shell: Arc::new(shell),
            abort: Mutex::new(None),
        }
    }

    fn abort_slot(&self) -> std::sync::MutexGuard<'_, Option<oneshot::Sender<()>>> {
        self.abort.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Runs `f` off the async runtime and logs the command's entry/exit at the IPC boundary -
/// what the frontend asked for and what it got back, independent of whatever `f` itself logs.
async fn blocking<T, F>(state: &AppState, name: &'static str, f: F) -> CmdResult<T>
where
    T: Send + 'static,
    F: FnOnce(&Shell) -> CmdResult<T> + Send + 'static,
{
    tracing::info!(command = name, "command invoked");
    let shell = state.shell.clone();
    let result = tauri::async_runtime::spawn_blocking(move || f(&shell))
        .await
        .map_err(|e| CmdError::io(format!("internal error: {e}")));
    let result = result.and_then(|r| r);
    if let Err(e) = &result {
        tracing::warn!(command = name, error = ?e, "command failed");
    }
    result
}

/// Runs one planning operation on its own thread, and returns as soon as it either finishes
/// or is cancelled.
///
/// A plain `spawn_blocking` isn't enough: ureq has no idle timeout, so a download stalled
/// inside `read()` never sees the cancel flag (CLAUDE.md "Known limitation"). On cancel this
/// returns immediately and abandons the thread; when it eventually finishes, `Shell::finish`
/// sees its ticket was cancelled and drops the plan (deleting the staged download) instead
/// of offering it. Planning never takes the commit lock, so an abandoned thread can't block
/// anything else.
async fn plan<F>(state: &AppState, name: &'static str, f: F) -> CmdResult<PlanView>
where
    F: FnOnce(&Shell, &Ticket) -> CmdResult<PlanView> + Send + 'static,
{
    tracing::info!(command = name, "command invoked");
    let shell = state.shell.clone();
    let ticket = shell.begin_planning();
    let (abort_tx, abort_rx) = oneshot::channel::<()>();
    // Replacing the previous sender drops it, which wakes (and cancels) any earlier waiter.
    *state.abort_slot() = Some(abort_tx);
    let (tx, rx) = oneshot::channel();
    std::thread::Builder::new()
        .name("esmm-plan".into())
        .spawn(move || {
            let _ = tx.send(f(&shell, &ticket));
        })?;
    let result = tokio::select! {
        result = rx => result.unwrap_or_else(|_| Err(CmdError::io("planning stopped unexpectedly"))),
        _ = abort_rx => Err(CmdError::cancelled()),
    };
    if let Err(e) = &result {
        // A cancelled plan is routine (superseded or user-cancelled), not worth a warning.
        if matches!(e, CmdError::Cancelled { .. }) {
            tracing::info!(command = name, "planning cancelled");
        } else {
            tracing::warn!(command = name, error = ?e, "command failed");
        }
    }
    result
}

// --- Catalog ---------------------------------------------------------------

#[tauri::command]
pub async fn load_catalog(state: State<'_, AppState>, refresh: bool) -> CmdResult<CatalogView> {
    blocking(&state, "load_catalog", move |s| s.load_catalog(refresh)).await
}

#[tauri::command]
pub async fn get_icon(state: State<'_, AppState>, url: String) -> CmdResult<String> {
    blocking(&state, "get_icon", move |s| s.icon(&url)).await
}

// --- Game installs -----------------------------------------------------------

#[tauri::command]
pub async fn list_installs(state: State<'_, AppState>, refresh: bool) -> CmdResult<InstallsView> {
    blocking(&state, "list_installs", move |s| {
        Ok(s.list_installs(refresh))
    })
    .await
}

#[tauri::command]
pub async fn select_install(state: State<'_, AppState>, key: String) -> CmdResult<InstallsView> {
    blocking(&state, "select_install", move |s| s.select_install(&key)).await
}

#[tauri::command]
pub async fn add_custom_install(
    state: State<'_, AppState>,
    config_dir: Option<String>,
    executable: Option<String>,
) -> CmdResult<InstallsView> {
    blocking(&state, "add_custom_install", move |s| {
        s.add_custom_install(config_dir, executable)
    })
    .await
}

#[tauri::command]
pub async fn remove_custom_install(
    state: State<'_, AppState>,
    key: String,
) -> CmdResult<InstallsView> {
    blocking(&state, "remove_custom_install", move |s| {
        s.remove_custom_install(&key)
    })
    .await
}

#[tauri::command]
pub async fn game_status(state: State<'_, AppState>) -> CmdResult<GameProcessView> {
    blocking(&state, "game_status", |s| Ok(s.game_status())).await
}

#[tauri::command]
pub async fn launch_game(state: State<'_, AppState>) -> CmdResult<()> {
    blocking(&state, "launch_game", |s| s.launch_game()).await
}

// --- State -------------------------------------------------------------------

#[tauri::command]
pub async fn get_state(state: State<'_, AppState>) -> CmdResult<ManagerState> {
    blocking(&state, "get_state", |s| s.get_state()).await
}

#[tauri::command]
pub async fn adopt_plugin(
    state: State<'_, AppState>,
    folder: String,
    catalog_name: String,
) -> CmdResult<()> {
    blocking(&state, "adopt_plugin", move |s| {
        s.adopt_plugin(&folder, &catalog_name)
    })
    .await
}

// --- Plans -------------------------------------------------------------------

#[tauri::command]
pub async fn plan_install(state: State<'_, AppState>, catalog_name: String) -> CmdResult<PlanView> {
    plan(&state, "plan_install", move |s, t| {
        s.plan_install(t, &catalog_name)
    })
    .await
}

#[tauri::command]
pub async fn plan_update(state: State<'_, AppState>, folder: String) -> CmdResult<PlanView> {
    plan(&state, "plan_update", move |s, t| s.plan_update(t, &folder)).await
}

#[tauri::command]
pub async fn plan_update_all(state: State<'_, AppState>) -> CmdResult<PlanView> {
    plan(&state, "plan_update_all", move |s, t| s.plan_update_all(t)).await
}

#[tauri::command]
pub async fn plan_enable(state: State<'_, AppState>, identity: String) -> CmdResult<PlanView> {
    plan(&state, "plan_enable", move |s, t| {
        s.plan_enable(t, &identity)
    })
    .await
}

#[tauri::command]
pub async fn plan_disable(state: State<'_, AppState>, identity: String) -> CmdResult<PlanView> {
    plan(&state, "plan_disable", move |s, t| {
        s.plan_disable(t, &identity)
    })
    .await
}

#[tauri::command]
pub async fn plan_uninstall(state: State<'_, AppState>, folder: String) -> CmdResult<PlanView> {
    plan(&state, "plan_uninstall", move |s, t| {
        s.plan_uninstall(t, &folder)
    })
    .await
}

#[tauri::command]
pub async fn plan_apply_profile(state: State<'_, AppState>, name: String) -> CmdResult<PlanView> {
    plan(&state, "plan_apply_profile", move |s, t| {
        s.plan_apply_profile(t, &name)
    })
    .await
}

#[tauri::command]
pub fn cancel_planning(state: State<'_, AppState>) {
    tracing::info!(command = "cancel_planning", "command invoked");
    state.shell.cancel_planning();
    if let Some(abort) = state.abort_slot().take() {
        let _ = abort.send(());
    }
}

#[tauri::command]
pub fn resolve_conflict(
    state: State<'_, AppState>,
    plan_id: u32,
    identity: String,
) -> CmdResult<PlanView> {
    tracing::info!(command = "resolve_conflict", "command invoked");
    state.shell.resolve_conflict(plan_id, &identity)
}

#[tauri::command]
pub fn discard_plan(state: State<'_, AppState>, plan_id: u32) {
    tracing::info!(command = "discard_plan", "command invoked");
    state.shell.discard_plan(plan_id);
}

#[tauri::command]
pub async fn commit_plan(
    state: State<'_, AppState>,
    plan_id: u32,
    override_issues: bool,
) -> CmdResult<CommitView> {
    blocking(&state, "commit_plan", move |s| {
        s.commit_plan(plan_id, override_issues)
    })
    .await
}

// --- Profiles ----------------------------------------------------------------

#[tauri::command]
pub async fn create_profile(state: State<'_, AppState>, name: String) -> CmdResult<String> {
    blocking(&state, "create_profile", move |s| s.create_profile(&name)).await
}

#[tauri::command]
pub async fn create_empty_profile(state: State<'_, AppState>, name: String) -> CmdResult<String> {
    blocking(&state, "create_empty_profile", move |s| {
        s.create_empty_profile(&name)
    })
    .await
}

#[tauri::command]
pub async fn rename_profile(
    state: State<'_, AppState>,
    old_name: String,
    new_name: String,
) -> CmdResult<String> {
    blocking(&state, "rename_profile", move |s| {
        s.rename_profile(&old_name, &new_name)
    })
    .await
}

#[tauri::command]
pub async fn export_profile(
    state: State<'_, AppState>,
    name: String,
    path: String,
) -> CmdResult<()> {
    blocking(&state, "export_profile", move |s| {
        s.export_profile(&name, std::path::Path::new(&path))
    })
    .await
}

#[tauri::command]
pub async fn import_profile(state: State<'_, AppState>, path: String) -> CmdResult<String> {
    blocking(&state, "import_profile", move |s| {
        s.import_profile(std::path::Path::new(&path))
    })
    .await
}

#[tauri::command]
pub async fn update_active_profile(state: State<'_, AppState>) -> CmdResult<()> {
    blocking(&state, "update_active_profile", |s| {
        s.update_active_profile()
    })
    .await
}

#[tauri::command]
pub async fn delete_profile(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    blocking(&state, "delete_profile", move |s| s.delete_profile(&name)).await
}
