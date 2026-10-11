//! The shell's side of the optional load-order feature (see `esmm_core::load_order`, which
//! lists everything to delete to remove the feature, this file included).
//!
//! Everything here goes through the `LoadOrderBackend` trait. The feature is off unless the
//! Settings switch `manage_load_order` is on: with it off nothing is planned, renamed or
//! numbered; the only thing still possible is restoring folders that were renamed earlier (so
//! switching the feature off never strands anyone).
//!
//! Plans are not kept in memory. A review dialog shows a plan and a short token that
//! identifies it; the commit builds the plan again from what is on disk and refuses if the
//! token no longer matches, so what is committed is always exactly what was reviewed.

use esmm_core::game_state::{self, GameProcess};
use esmm_core::install;
use esmm_core::load_order::{
    self, ChangePlan, LoadOrderBackend, LoadOrderError, OverlapPlugin, Paths, Recovery,
    WinnerChange,
};
use esmm_core::manager::InstalledPlugin;
use esmm_core::overlap::{self, PluginIndex};
use sha2::{Digest, Sha256};

use super::{Shell, Snapshot, lock};
use crate::error::{CmdError, CmdResult};
use crate::load_order_views::*;
use crate::settings::{InstallPaths, Settings};

fn paths_of(paths: &InstallPaths) -> Paths<'_> {
    Paths::new(&paths.config_dir, &paths.records)
}

fn cmd_err(e: LoadOrderError) -> CmdError {
    match e {
        LoadOrderError::GameRunning => CmdError::game_running(),
        LoadOrderError::Io(m)
        | LoadOrderError::Records(m)
        | LoadOrderError::Journal(m)
        | LoadOrderError::Unrecoverable(m) => CmdError::io(m),
        other => CmdError::invalid(other.to_string()),
    }
}

/// A short fingerprint of what a plan would rename, so a commit can check it is committing
/// what was reviewed.
fn plan_token(plan: &ChangePlan) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{:?}\n", plan.kind));
    for r in &plan.renames {
        hasher.update(r.from.as_bytes());
        hasher.update([0]);
        hasher.update(r.to.as_bytes());
        hasher.update(b"\n");
    }
    hasher.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn short_hash(text: &str) -> String {
    Sha256::digest(text.as_bytes())[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl Shell {
    /// Finishes or undoes a renaming run that was interrupted (a crash, a power cut). Runs
    /// whenever the install is read, before any plugin folder is scanned, whether or not the
    /// feature is switched on: a half-done rename must never be left behind.
    pub(super) fn recover_interrupted(&self, paths: &InstallPaths) {
        // Any command can reach this; the core serialises it against a commit in progress.
        let game = (self.deps.detect_game)();
        match load_order::default_backend().recover(&paths_of(paths), game) {
            Ok(Recovery::Nothing) => {}
            Ok(Recovery::Finished { renames }) => {
                tracing::warn!(renames, "finished an interrupted load-order change");
            }
            Ok(Recovery::RolledBack { renames, reason }) => {
                tracing::warn!(renames, %reason, "undid an interrupted load-order change");
            }
            Err(e) => {
                tracing::warn!(error = %e, "couldn't recover an interrupted load-order change")
            }
        }
    }

    fn load_order_enabled(&self) -> bool {
        Settings::load(&self.deps.data_dir).manage_load_order
    }

    fn require_load_order(&self) -> CmdResult<()> {
        if self.load_order_enabled() {
            Ok(())
        } else {
            Err(CmdError::invalid(
                "Load order management is switched off. Turn it on in Settings first.",
            ))
        }
    }

    pub fn load_order_state(&self) -> CmdResult<LoadOrderState> {
        let _op = lock(&self.op_lock);
        let enabled = self.load_order_enabled();
        let snap = match self.snapshot() {
            Ok((_, snap)) => snap,
            // No game install yet: only the switch itself can be shown.
            Err(CmdError::NoInstall { .. }) => return Ok(LoadOrderState::empty(enabled, 0)),
            Err(e) => return Err(e),
        };
        let layout = load_order::default_backend().layout(&snap.installed);
        let renamed_count = layout.restorable.len() as u32;
        if !enabled {
            return Ok(LoadOrderState::empty(false, renamed_count));
        }
        let enabled = |identity: &str| game_state::effective_enabled(&snap.states, identity);
        Ok(LoadOrderState {
            enabled: true,
            plugins: layout
                .ordered
                .iter()
                .map(|p| LoadOrderPluginView {
                    folder: p.folder.clone(),
                    identity: p.identity.clone(),
                    original_folder: p.original_folder.clone(),
                    number: p.number,
                    enabled: enabled(&p.identity),
                    requires: p.requires.clone(),
                })
                .collect(),
            excluded: layout
                .excluded
                .iter()
                .map(|e| LoadOrderExcludedView {
                    folder: e.folder.clone(),
                    identity: e.identity.clone(),
                    reasons: e.reasons.iter().copied().map(Into::into).collect(),
                    still_renamed: e.still_renamed,
                    enabled: enabled(&e.identity),
                })
                .collect(),
            renamed_count,
        })
    }

    /// Flips the Settings switch. Switching off changes no folder by itself; the reply says
    /// how many are still renamed so the UI can offer to restore them.
    pub fn load_order_set_enabled(&self, enabled: bool) -> CmdResult<LoadOrderSwitch> {
        let _op = lock(&self.op_lock);
        let mut settings = Settings::load(&self.deps.data_dir);
        settings.manage_load_order = enabled;
        settings.save(&self.deps.data_dir)?;
        tracing::info!(enabled, "load order management switched");
        let renamed_count = self
            .snapshot()
            .map(|(_, snap)| {
                load_order::default_backend()
                    .layout(&snap.installed)
                    .restorable
                    .len() as u32
            })
            .unwrap_or(0);
        Ok(LoadOrderSwitch {
            enabled,
            renamed_count,
        })
    }

    /// The current order changed as little as possible so required plugins load first.
    pub fn load_order_suggest(&self) -> CmdResult<LoadOrderSuggestion> {
        self.require_load_order()?;
        let _op = lock(&self.op_lock);
        let (_, snap) = self.snapshot()?;
        let layout = load_order::default_backend().layout(&snap.installed);
        let suggestion = load_order::suggest_order(&layout);
        Ok(LoadOrderSuggestion {
            order: suggestion.order,
            fixed: suggestion.fixed.iter().map(Into::into).collect(),
        })
    }

    /// The live banner while the user rearranges: dependency problems and which overlaps would
    /// change winner. Changes nothing.
    pub fn load_order_preview(&self, wanted: Vec<String>) -> CmdResult<LoadOrderPreview> {
        self.require_load_order()?;
        let _op = lock(&self.op_lock);
        let (_, snap) = self.snapshot()?;
        let backend = load_order::default_backend();
        let layout = backend.layout(&snap.installed);
        let dependency_notes = load_order::dependency_violations(&layout, &wanted)
            .iter()
            .map(Into::into)
            .collect();
        match backend.plan_apply(&paths_of(&snap.paths), &snap.installed, &wanted) {
            Ok(plan) => Ok(LoadOrderPreview {
                dependency_notes,
                changes: overlap_changes(&snap, &plan)
                    .iter()
                    .map(Into::into)
                    .collect(),
                rename_count: plan.renames.len() as u32,
                problem: None,
            }),
            Err(LoadOrderError::OrderMismatch) => Err(cmd_err(LoadOrderError::OrderMismatch)),
            Err(e) => Ok(LoadOrderPreview {
                dependency_notes,
                changes: Vec::new(),
                rename_count: 0,
                problem: Some(e.to_string()),
            }),
        }
    }

    pub fn load_order_plan_apply(&self, wanted: Vec<String>) -> CmdResult<LoadOrderPlanView> {
        self.require_load_order()?;
        let _op = lock(&self.op_lock);
        let (_, snap) = self.snapshot()?;
        let plan = load_order::default_backend()
            .plan_apply(&paths_of(&snap.paths), &snap.installed, &wanted)
            .map_err(cmd_err)?;
        tracing::info!(renames = plan.renames.len(), "load order apply planned");
        let changes = overlap_changes(&snap, &plan);
        Ok(LoadOrderPlanView::new(&plan, plan_token(&plan), &changes))
    }

    /// Allowed whether or not the switch is on.
    pub fn load_order_plan_restore(&self) -> CmdResult<LoadOrderPlanView> {
        let _op = lock(&self.op_lock);
        let (_, snap) = self.snapshot()?;
        let plan = load_order::default_backend()
            .plan_restore(&paths_of(&snap.paths), &snap.installed)
            .map_err(cmd_err)?;
        tracing::info!(renames = plan.renames.len(), "load order restore planned");
        let changes = overlap_changes(&snap, &plan);
        Ok(LoadOrderPlanView::new(&plan, plan_token(&plan), &changes))
    }

    pub fn load_order_commit_apply(
        &self,
        wanted: Vec<String>,
        token: &str,
    ) -> CmdResult<LoadOrderCommitView> {
        self.require_load_order()?;
        self.load_order_commit(token, |backend, snap| {
            backend.plan_apply(&paths_of(&snap.paths), &snap.installed, &wanted)
        })
    }

    pub fn load_order_commit_restore(&self, token: &str) -> CmdResult<LoadOrderCommitView> {
        self.load_order_commit(token, |backend, snap| {
            backend.plan_restore(&paths_of(&snap.paths), &snap.installed)
        })
    }

    fn load_order_commit(
        &self,
        token: &str,
        make_plan: impl FnOnce(&dyn LoadOrderBackend, &Snapshot) -> Result<ChangePlan, LoadOrderError>,
    ) -> CmdResult<LoadOrderCommitView> {
        let _op = lock(&self.op_lock);
        let game = (self.deps.detect_game)();
        if game == GameProcess::Running {
            return Err(CmdError::game_running());
        }
        let (_, snap) = self.snapshot()?;
        let backend = load_order::default_backend();
        let plan = make_plan(&backend, &snap).map_err(cmd_err)?;
        if plan_token(&plan) != token {
            return Err(CmdError::invalid(
                "The plugin folders changed since you reviewed this. Review the change again.",
            ));
        }
        let result = backend.commit(&paths_of(&snap.paths), &plan, game);
        // Any plan waiting in the main flow may refer to folders that now have other names, so
        // drop it unless the commit was refused before anything was touched.
        let touched = !matches!(
            &result,
            Err(LoadOrderError::GameRunning
                | LoadOrderError::JournalPending
                | LoadOrderError::Missing(_)
                | LoadOrderError::Changed(_)
                | LoadOrderError::Collision { .. })
        );
        if touched {
            self.reset_plans();
        }
        let outcome = result.map_err(|e| {
            tracing::error!(error = %e, "load order commit failed");
            cmd_err(e)
        })?;
        tracing::info!(renamed = outcome.renamed, "load order committed");
        Ok(LoadOrderCommitView {
            renamed: outcome.renamed as u32,
            backup: outcome.backup.map(|p| p.display().to_string()),
        })
    }
}

/// The overlaps (among enabled plugins) whose winner `plan` would change. Each plugin is read
/// once and its index cached beside the install's records (keyed by the plugin's original
/// folder name, so a rename doesn't invalidate it); a plugin that can't be read is left out.
fn overlap_changes(snap: &Snapshot, plan: &ChangePlan) -> Vec<WinnerChange> {
    if plan.renames.is_empty() {
        return Vec::new();
    }
    let plugins_dir = install::plugins_dir(&snap.paths.config_dir);
    let cache_dir = snap.paths.records.with_file_name("load-order-index");
    let mut indexed: Vec<(&InstalledPlugin, PluginIndex)> = Vec::new();
    for plugin in snap
        .installed
        .iter()
        .filter(|p| game_state::effective_enabled(&snap.states, &p.identity))
    {
        let base = plugin
            .record
            .as_ref()
            .and_then(|r| r.original_folder.as_deref())
            .unwrap_or(&plugin.folder);
        let cache = cache_dir.join(format!("{}.json", short_hash(base)));
        match overlap::load_or_index(&plugins_dir.join(&plugin.folder), &cache) {
            Ok(index) => indexed.push((plugin, index)),
            Err(e) => {
                tracing::warn!(folder = plugin.folder, error = %e, "couldn't read a plugin for the overlap check")
            }
        }
    }
    let list: Vec<OverlapPlugin> = indexed
        .iter()
        .map(|(plugin, index)| OverlapPlugin {
            folder: &plugin.folder,
            identity: &plugin.identity,
            index,
        })
        .collect();
    load_order::winner_changes(&load_order::default_backend(), &list, plan)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::time::SystemTime;

    use esmm_core::catalog::{CatalogEntry, CatalogFetch, FetchSource};
    use esmm_core::download::Downloaded;
    use esmm_core::game_install::GameInstall;
    use esmm_core::manager::Fetcher;
    use esmm_core::records::{self, InstallRecord, InstallRecords};

    use super::*;
    use crate::shell::Deps;

    thread_local! {
        static GAME_RUNNING: Cell<bool> = const { Cell::new(false) };
    }

    fn fake_detect_game() -> GameProcess {
        if GAME_RUNNING.with(Cell::get) {
            GameProcess::Running
        } else {
            GameProcess::NotRunning
        }
    }

    struct NoFetch;

    impl Fetcher for NoFetch {
        fn fetch(
            &self,
            _entry: &CatalogEntry,
            _dest: &Path,
            _progress: &mut dyn FnMut(u64, Option<u64>),
            _cancel: &AtomicBool,
        ) -> Result<Downloaded, String> {
            Err("no downloads in these tests".into())
        }
    }

    struct World {
        _dir: tempfile::TempDir,
        config: PathBuf,
        shell: Shell,
    }

    impl World {
        fn new() -> Self {
            GAME_RUNNING.with(|g| g.set(false));
            let dir = tempfile::tempdir().unwrap();
            let config = dir.path().join("endless-sky");
            fs::create_dir_all(config.join("plugins")).unwrap();
            let deps = Deps {
                data_dir: dir.path().join("app-data"),
                cache_dir: dir.path().join("app-cache"),
                env: None,
                fetcher: Arc::new(NoFetch),
                fetch_catalog: Box::new(|_| {
                    Ok(CatalogFetch {
                        entries: Vec::new(),
                        source: FetchSource::Fresh,
                        fetched_at: SystemTime::now(),
                    })
                }),
                fetch_icon: Box::new(|cache_dir, _| Ok(cache_dir.join("none.png"))),
                detect_game: fake_detect_game,
                query_version: |_: &GameInstall| Some("0.10.16".into()),
                launch: |_: &GameInstall| Ok(()),
                progress: Arc::new(|_| {}),
            };
            let shell = Shell::new(deps);
            shell
                .add_custom_install(Some(config.display().to_string()), None)
                .unwrap();
            World {
                _dir: dir,
                config,
                shell,
            }
        }

        fn paths(&self) -> InstallPaths {
            InstallPaths::new(&self.shell.deps.data_dir, &self.config)
        }

        fn plugins(&self) -> PathBuf {
            self.config.join("plugins")
        }

        /// A plugin the manager installed: folder, `plugin.txt` name, record with a hash.
        fn add(&self, folder: &str, name: Option<&str>) {
            let dir = self.plugins().join(folder);
            fs::create_dir_all(dir.join("data")).unwrap();
            fs::write(dir.join("data/x.txt"), "ship \"X\"\n").unwrap();
            if let Some(name) = name {
                fs::write(dir.join("plugin.txt"), format!("name \"{name}\"\n")).unwrap();
            }
            let path = self.paths().records;
            let mut records: InstallRecords = records::load(&path).unwrap();
            records.insert(
                folder.to_string(),
                InstallRecord {
                    catalog_name: folder.to_string(),
                    folder: folder.to_string(),
                    identity: name.unwrap_or(folder).to_string(),
                    version: "1".into(),
                    source_url: "https://example.com/x.zip".into(),
                    sha256: "hash".into(),
                    original_folder: None,
                },
            );
            records::save(&path, &records).unwrap();
        }

        fn folders(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(self.plugins())
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        }
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_feature_is_off_by_default_and_then_nothing_is_offered_or_renamed() {
        let w = World::new();
        w.add("Alpha", Some("Alpha P"));
        assert!(!Settings::load(&w.shell.deps.data_dir).manage_load_order);

        let state = w.shell.load_order_state().unwrap();
        assert!(!state.enabled);
        assert!(state.plugins.is_empty() && state.excluded.is_empty());
        assert_eq!(state.renamed_count, 0);

        for result in [
            w.shell.load_order_plan_apply(strs(&["Alpha"])).map(|_| ()),
            w.shell.load_order_suggest().map(|_| ()),
            w.shell.load_order_preview(strs(&["Alpha"])).map(|_| ()),
            w.shell
                .load_order_commit_apply(strs(&["Alpha"]), "x")
                .map(|_| ()),
        ] {
            assert!(
                matches!(result, Err(CmdError::Invalid { .. })),
                "{result:?}"
            );
        }
        assert_eq!(w.folders(), ["Alpha"], "no folder was renamed or numbered");
    }

    #[test]
    fn with_the_switch_on_an_order_can_be_reviewed_applied_and_restored() {
        let w = World::new();
        w.add("Alpha", Some("Alpha P"));
        w.add("Beta", Some("Beta P"));
        w.add("Anon", None);
        assert!(w.shell.load_order_set_enabled(true).unwrap().enabled);

        let state = w.shell.load_order_state().unwrap();
        assert_eq!(
            state
                .plugins
                .iter()
                .map(|p| p.folder.as_str())
                .collect::<Vec<_>>(),
            ["Alpha", "Beta"]
        );
        assert_eq!(state.excluded.len(), 1);
        assert_eq!(state.excluded[0].folder, "Anon");
        assert_eq!(
            state.excluded[0].reasons,
            [LoadOrderExcludeReason::NoPluginName]
        );

        let plan = w
            .shell
            .load_order_plan_apply(strs(&["Beta", "Alpha"]))
            .unwrap();
        assert_eq!(plan.renames.len(), 2);
        assert_eq!(plan.renames[0].to, "010-Beta");
        // Reviewing changed nothing.
        assert_eq!(w.folders(), ["Alpha", "Anon", "Beta"]);

        let done = w
            .shell
            .load_order_commit_apply(strs(&["Beta", "Alpha"]), &plan.token)
            .unwrap();
        assert_eq!(done.renamed, 2);
        assert_eq!(w.folders(), ["010-Beta", "020-Alpha", "Anon"]);

        // The rest of the app sees the same plugins, by the same identities.
        let state = w.shell.get_state().unwrap();
        let mut identities: Vec<&str> = state.plugins.iter().map(|p| p.identity.as_str()).collect();
        identities.sort();
        assert_eq!(identities, ["Alpha P", "Anon", "Beta P"]);

        // Switching off reports what is still renamed, and restoring still works.
        let off = w.shell.load_order_set_enabled(false).unwrap();
        assert_eq!((off.enabled, off.renamed_count), (false, 2));
        let restore = w.shell.load_order_plan_restore().unwrap();
        assert_eq!(restore.renames.len(), 2);
        w.shell.load_order_commit_restore(&restore.token).unwrap();
        assert_eq!(w.folders(), ["Alpha", "Anon", "Beta"]);
        assert_eq!(w.shell.load_order_state().unwrap().renamed_count, 0);
    }

    #[test]
    fn a_commit_for_a_different_plan_than_the_one_reviewed_is_refused() {
        let w = World::new();
        w.add("Alpha", Some("Alpha P"));
        w.add("Beta", Some("Beta P"));
        w.shell.load_order_set_enabled(true).unwrap();
        let plan = w
            .shell
            .load_order_plan_apply(strs(&["Alpha", "Beta"]))
            .unwrap();
        let err = w
            .shell
            .load_order_commit_apply(strs(&["Beta", "Alpha"]), &plan.token)
            .unwrap_err();
        assert!(matches!(err, CmdError::Invalid { .. }), "{err:?}");
        assert_eq!(w.folders(), ["Alpha", "Beta"]);
    }

    #[test]
    fn nothing_is_renamed_while_the_game_runs() {
        let w = World::new();
        w.add("Alpha", Some("Alpha P"));
        w.shell.load_order_set_enabled(true).unwrap();
        let plan = w.shell.load_order_plan_apply(strs(&["Alpha"])).unwrap();
        GAME_RUNNING.with(|g| g.set(true));
        let err = w
            .shell
            .load_order_commit_apply(strs(&["Alpha"]), &plan.token)
            .unwrap_err();
        assert!(matches!(err, CmdError::GameRunning { .. }), "{err:?}");
        assert_eq!(w.folders(), ["Alpha"]);
    }

    #[test]
    fn a_rename_that_would_clash_is_explained_in_the_preview_and_refused_by_the_plan() {
        let w = World::new();
        w.add("Alpha", Some("Alpha P"));
        fs::create_dir_all(w.plugins().join("010-Alpha/data")).unwrap();
        w.shell.load_order_set_enabled(true).unwrap();
        let preview = w.shell.load_order_preview(strs(&["Alpha"])).unwrap();
        assert!(preview.problem.as_deref().unwrap().contains("010-Alpha"));
        assert!(w.shell.load_order_plan_apply(strs(&["Alpha"])).is_err());
    }

    #[test]
    fn an_interrupted_run_is_finished_before_the_plugins_are_read() {
        let w = World::new();
        w.add("Alpha", Some("Alpha P"));
        w.add("Beta", Some("Beta P"));
        w.shell.load_order_set_enabled(true).unwrap();

        // Simulate a crash: the journal says both folders are being renamed, only one was.
        let paths = w.paths();
        let journal = paths.records.with_file_name(load_order::JOURNAL_FILE);
        let rename = |from: &str, to: &str, original: &str, identity: &str| {
            format!(
                r#"{{"from":"{from}","to":"{to}","identity":"{identity}","from_original":null,"to_original":"{original}"}}"#
            )
        };
        fs::write(
            &journal,
            format!(
                r#"{{"format":1,"renames":[{},{}]}}"#,
                rename("Alpha", "010-Alpha", "Alpha", "Alpha P"),
                rename("Beta", "020-Beta", "Beta", "Beta P")
            ),
        )
        .unwrap();
        fs::rename(w.plugins().join("Alpha"), w.plugins().join("010-Alpha")).unwrap();

        // Just reading the state recovers (and the renamed plugin is still found by its record).
        let state = w.shell.get_state().unwrap();
        assert_eq!(w.folders(), ["010-Alpha", "020-Beta"]);
        assert!(!journal.exists());
        assert!(state.unmanaged.is_empty());
        let records = records::load(&paths.records).unwrap();
        assert_eq!(records["020-Beta"].original_folder.as_deref(), Some("Beta"));
    }

    #[test]
    fn a_pending_plan_in_the_main_flow_is_dropped_when_folders_are_renamed() {
        let w = World::new();
        w.add("Alpha", Some("Alpha P"));
        w.shell.load_order_set_enabled(true).unwrap();
        let ticket = w.shell.begin_planning();
        let plan = w.shell.plan_disable(&ticket, "Alpha P").unwrap();
        let reviewed = w.shell.load_order_plan_apply(strs(&["Alpha"])).unwrap();
        w.shell
            .load_order_commit_apply(strs(&["Alpha"]), &reviewed.token)
            .unwrap();
        assert!(matches!(
            w.shell.commit_plan(plan.plan_id, false),
            Err(CmdError::PlanExpired { .. })
        ));
    }

    mod ipc {
        use tauri::Manager;
        use tauri::ipc::{CallbackFn, InvokeBody};
        use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};
        use tauri::webview::InvokeRequest;

        use super::*;
        use crate::commands::AppState;

        fn invoke(
            webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
            cmd: &str,
            args: serde_json::Value,
        ) -> Result<serde_json::Value, serde_json::Value> {
            get_ipc_response(
                webview,
                InvokeRequest {
                    cmd: cmd.into(),
                    callback: CallbackFn(0),
                    error: CallbackFn(1),
                    url: "tauri://localhost".parse().unwrap(),
                    body: InvokeBody::Json(args),
                    headers: Default::default(),
                    invoke_key: INVOKE_KEY.to_string(),
                },
            )
            .map(|body| body.deserialize::<serde_json::Value>().unwrap())
        }

        /// The argument names and payload shapes exactly as the frontend sends and reads them.
        #[test]
        fn load_order_commands_through_ipc() {
            let w = World::new();
            w.add("Alpha", Some("Alpha P"));
            w.add("Beta", Some("Beta P"));
            let plugins = w.plugins();
            let app = crate::with_commands(mock_builder())
                .build(mock_context(noop_assets()))
                .unwrap();
            let dir = w._dir;
            app.manage(AppState::new(w.shell));
            let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
                .build()
                .unwrap();
            let call = |cmd: &str, args: serde_json::Value| invoke(&webview, cmd, args);

            let state = call("load_order_state", serde_json::json!({})).unwrap();
            assert_eq!(state["enabled"], false);
            assert_eq!(state["plugins"], serde_json::json!([]));

            let on = call(
                "load_order_set_enabled",
                serde_json::json!({"enabled": true}),
            )
            .unwrap();
            assert_eq!(on, serde_json::json!({"enabled": true, "renamedCount": 0}));
            let state = call("load_order_state", serde_json::json!({})).unwrap();
            assert_eq!(state["plugins"][0]["folder"], "Alpha");
            assert_eq!(state["plugins"][0]["number"], serde_json::Value::Null);
            assert_eq!(state["plugins"][0]["originalFolder"], "Alpha");

            let wanted = serde_json::json!(["Beta", "Alpha"]);
            let preview =
                call("load_order_preview", serde_json::json!({"wanted": wanted})).unwrap();
            assert_eq!(preview["renameCount"], 2);
            assert_eq!(preview["problem"], serde_json::Value::Null);
            let suggestion = call("load_order_suggest", serde_json::json!({})).unwrap();
            assert_eq!(suggestion["order"], serde_json::json!(["Alpha", "Beta"]));

            let plan = call(
                "load_order_plan_apply",
                serde_json::json!({"wanted": wanted}),
            )
            .unwrap();
            assert_eq!(plan["kind"], "apply");
            assert_eq!(
                plan["renames"][0],
                serde_json::json!({"from": "Beta", "to": "010-Beta", "identity": "Beta P"})
            );
            let token = plan["token"].as_str().unwrap().to_string();

            let err = call(
                "load_order_commit_apply",
                serde_json::json!({"wanted": wanted, "token": "stale"}),
            )
            .unwrap_err();
            assert_eq!(err["kind"], "invalid");

            let done = call(
                "load_order_commit_apply",
                serde_json::json!({"wanted": wanted, "token": token}),
            )
            .unwrap();
            assert_eq!(done["renamed"], 2);
            assert!(plugins.join("010-Beta").is_dir() && plugins.join("020-Alpha").is_dir());

            let restore = call("load_order_plan_restore", serde_json::json!({})).unwrap();
            assert_eq!(restore["kind"], "restore");
            let token = restore["token"].as_str().unwrap().to_string();
            call(
                "load_order_commit_restore",
                serde_json::json!({"token": token}),
            )
            .unwrap();
            assert!(plugins.join("Alpha").is_dir() && plugins.join("Beta").is_dir());

            let err = call(
                "load_order_plan_apply",
                serde_json::json!({"order": wanted}),
            )
            .unwrap_err();
            assert!(err.as_str().is_some_and(|e| e.contains("wanted")), "{err}");
            drop(dir);
        }
    }
}
