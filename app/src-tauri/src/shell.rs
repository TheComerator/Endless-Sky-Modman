//! The Tauri-independent service behind every command (decision K).
//!
//! Commands in `commands.rs` are thin async wrappers; everything with behavior lives here so
//! it can be tested without a webview, against temp dirs and a fake `Fetcher`.
//!
//! **What lives where.** The game's own files and the manager's JSON files are the source of
//! truth and are re-read on every call (they're tiny, and the game rewrites `plugins.txt`
//! behind our back). The shell holds in memory only what can't be re-read cheaply or can't
//! be serialized: the catalog snapshot, cached game versions, the one pending plan (plans
//! own staged downloads in temp dirs), and the in-flight planning ticket.
//!
//! **Two-phase plan/commit.** Every mutation is `plan_*` (returns a [`PlanView`] with all
//! issues and notes, nothing written) then [`Shell::commit_plan`] (by plan id, with an
//! explicit override flag). At most one plan is pending: starting another supersedes it,
//! which drops its staged downloads. A commit refused for blocking issues or a running game
//! leaves the plan pending so the user can override or retry without re-downloading.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use base64::Engine;
use esmm_core::catalog::{self, CatalogEntry, CatalogError, CatalogFetch, FetchSource};
use esmm_core::game_install::{self, DetectEnv, GameInstall, Launch};
use esmm_core::game_state::{self, GameProcess};
use esmm_core::install;
use esmm_core::manager::{
    self, CommitContext, DisablePlan, EnablePlan, Fetcher, HttpFetcher, InstallPlan,
    InstalledPlugin, PlanContext, PlanStep, UninstallPlan, UpdatePlan,
};
use esmm_core::plugin_state::PluginStates;
use esmm_core::profiles::{self, ProfileStore};
use esmm_core::records::{self, InstallRecord, InstallRecords};
use esmm_core::resolve::{self, CatalogMatch, Issue, Note, PlannedPlugin};

use crate::error::{CmdError, CmdResult};
use crate::fetcher::{PlanFetcher, ProgressSink};
use crate::settings::{CustomInstall, InstallPaths, Settings, install_key};
use crate::views::*;

type CatalogFn = dyn Fn(&Path) -> Result<CatalogFetch, CatalogError> + Send + Sync;
type IconFn = dyn Fn(&Path, &str) -> Result<PathBuf, CatalogError> + Send + Sync;

/// Everything the shell reads from or does to the outside world, injectable for tests.
pub struct Deps {
    /// The manager's app-data dir (settings, records, profiles).
    pub data_dir: PathBuf,
    /// The manager's cache dir; the catalog/icon cache lives in `<cache_dir>/catalog`.
    pub cache_dir: PathBuf,
    /// `None` when the home dir is unknown: only user-added installs with a config dir work.
    pub env: Option<DetectEnv>,
    pub fetcher: Arc<dyn Fetcher + Send + Sync>,
    pub fetch_catalog: Box<CatalogFn>,
    pub fetch_icon: Box<IconFn>,
    pub detect_game: fn() -> GameProcess,
    pub query_version: fn(&GameInstall) -> Option<String>,
    pub launch: fn(&GameInstall) -> std::io::Result<()>,
    pub progress: ProgressSink,
}

impl Deps {
    pub fn production(data_dir: PathBuf, cache_dir: PathBuf, progress: ProgressSink) -> Self {
        Deps {
            data_dir,
            cache_dir,
            env: DetectEnv::from_system(),
            fetcher: Arc::new(HttpFetcher::default()),
            fetch_catalog: Box::new(catalog::fetch_cached),
            fetch_icon: Box::new(catalog::cached_icon),
            detect_game: game_state::detect_game_process,
            query_version: game_install::query_install_version,
            launch: launch_detached,
            progress,
        }
    }
}

/// Starts the game and reaps it on a background thread, so it never lingers as a zombie
/// while the manager stays open.
fn launch_detached(install: &GameInstall) -> std::io::Result<()> {
    let mut child = game_install::launch(install)?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Identifies one planning run; its cancel flag reaches the download loop via [`PlanFetcher`].
#[derive(Debug, Clone)]
pub struct Ticket {
    pub id: u32,
    pub cancel: Arc<AtomicBool>,
}

// At most one of these exists at a time, so variant size doesn't matter.
#[allow(clippy::large_enum_variant)]
enum PendingPlan {
    Install {
        plan: InstallPlan,
        root_catalog_name: String,
    },
    Update {
        plan: UpdatePlan,
        step: StepView,
    },
    /// One combined plan over every outdated plugin (the "update all" button), so all of
    /// their issues/notes are reviewed together before anything commits - not several
    /// separate plan/commit round trips, and not a silent batch that skips past one
    /// plugin's blocking issue. `issues`/`notes`/`steps` are each plan's own, flattened
    /// once at plan time (same pattern as `ApplyProfile`), since there's no single owned
    /// plan struct here to borrow them from.
    UpdateAll {
        plans: Vec<UpdatePlan>,
        steps: Vec<StepView>,
        issues: Vec<Issue>,
        notes: Vec<Note>,
    },
    Enable(EnablePlan),
    Disable(DisablePlan),
    Uninstall(UninstallPlan),
    ApplyProfile {
        name: String,
        states: PluginStates,
        steps: Vec<StepView>,
        issues: Vec<Issue>,
        notes: Vec<Note>,
    },
}

struct Pending {
    id: u32,
    /// The install the plan was built against: commit always acts on it, even if the
    /// selection changed in between (selecting another install discards the plan anyway).
    paths: InstallPaths,
    target: String,
    plan: PendingPlan,
    fixes: Vec<RequirementFixView>,
    missing: Vec<MissingView>,
    game_version: Option<String>,
    unmanaged_target: bool,
    /// For install/enable plans: the identity the plan is already about, excluded from
    /// `resolvable_conflicts` (disabling it "to resolve" its own conflict would amount to not
    /// doing the plan at all). `None` for a profile switch, which has no single such identity
    /// -- either side of a conflict there is a legitimate target.
    root_identity: Option<String>,
}

impl Pending {
    fn issues(&self) -> &[Issue] {
        match &self.plan {
            PendingPlan::Install { plan, .. } => &plan.issues,
            PendingPlan::Update { plan, .. } => &plan.issues,
            PendingPlan::UpdateAll { issues, .. } => issues,
            PendingPlan::Enable(plan) => &plan.issues,
            PendingPlan::Disable(plan) => &plan.issues,
            PendingPlan::Uninstall(plan) => &plan.issues,
            PendingPlan::ApplyProfile { issues, .. } => issues,
        }
    }

    fn notes(&self) -> &[Note] {
        match &self.plan {
            PendingPlan::Install { plan, .. } => &plan.notes,
            PendingPlan::Update { plan, .. } => &plan.notes,
            PendingPlan::UpdateAll { notes, .. } => notes,
            PendingPlan::Enable(plan) => &plan.notes,
            PendingPlan::ApplyProfile { notes, .. } => notes,
            PendingPlan::Disable(_) | PendingPlan::Uninstall(_) => &[],
        }
    }

    fn kind(&self) -> PlanKind {
        match &self.plan {
            PendingPlan::Install { .. } => PlanKind::Install,
            PendingPlan::Update { .. } => PlanKind::Update,
            PendingPlan::UpdateAll { .. } => PlanKind::UpdateAll,
            PendingPlan::Enable(_) => PlanKind::Enable,
            PendingPlan::Disable(_) => PlanKind::Disable,
            PendingPlan::Uninstall(_) => PlanKind::Uninstall,
            PendingPlan::ApplyProfile { .. } => PlanKind::ApplyProfile,
        }
    }

    fn steps(&self) -> Vec<StepView> {
        match &self.plan {
            PendingPlan::Install {
                plan,
                root_catalog_name,
            } => plan
                .steps
                .iter()
                .map(|step| match step {
                    PlanStep::Install(staged) => StepView::Install {
                        catalog_name: staged.catalog_name.clone(),
                        identity: staged.identity.clone(),
                        folder: staged.folder.clone(),
                        version: staged.version.clone(),
                        dependency: &staged.catalog_name != root_catalog_name,
                    },
                    PlanStep::Enable(identity) => StepView::Enable {
                        identity: identity.clone(),
                    },
                    PlanStep::Disable(identity) => StepView::Disable {
                        identity: identity.clone(),
                    },
                })
                .collect(),
            PendingPlan::Update { plan, step } => {
                let mut steps = requirement_steps(plan);
                steps.push(step.clone());
                steps
            }
            PendingPlan::UpdateAll { steps, .. } => steps.clone(),
            PendingPlan::Enable(plan) => vec![StepView::Enable {
                identity: plan.identity.clone(),
            }],
            PendingPlan::Disable(plan) => vec![StepView::Disable {
                identity: plan.identity.clone(),
            }],
            PendingPlan::Uninstall(plan) => vec![StepView::Uninstall {
                folder: plan.folder.clone(),
                identity: plan.identity.clone(),
            }],
            PendingPlan::ApplyProfile { steps, .. } => steps.clone(),
        }
    }

    fn resolvable_conflicts(&self) -> Vec<String> {
        if !matches!(
            self.plan,
            PendingPlan::Install { .. } | PendingPlan::Enable(_) | PendingPlan::ApplyProfile { .. }
        ) {
            return Vec::new();
        }
        let mut out = BTreeSet::new();
        for issue in self.issues() {
            if let Issue::Conflict { a, b } = issue {
                for side in [a, b] {
                    if Some(side) != self.root_identity.as_ref() {
                        out.insert(side.clone());
                    }
                }
            }
        }
        out.into_iter().collect()
    }

    fn view(&self) -> PlanView {
        PlanView {
            plan_id: self.id,
            kind: self.kind(),
            target: self.target.clone(),
            steps: self.steps(),
            issues: issue_views(self.issues()),
            notes: self.notes().iter().map(NoteView::from).collect(),
            fixes: self.fixes.clone(),
            resolvable_conflicts: self.resolvable_conflicts(),
            missing: self.missing.clone(),
            game_version: self.game_version.clone(),
            unmanaged_target: self.unmanaged_target,
        }
    }
}

/// The selected install plus everything read from disk for it, for one operation.
struct Snapshot {
    paths: InstallPaths,
    records: InstallRecords,
    installed: Vec<InstalledPlugin>,
    states: PluginStates,
    catalog: Arc<CatalogFetch>,
    game_version: Option<String>,
}

impl Snapshot {
    fn identities(&self) -> BTreeSet<String> {
        self.installed.iter().map(|p| p.identity.clone()).collect()
    }

    fn plan_ctx<'a>(&'a self, fetcher: &'a dyn Fetcher) -> PlanContext<'a> {
        PlanContext {
            catalog: &self.catalog.entries,
            installed: &self.installed,
            records: &self.records,
            states: &self.states,
            game_version: self.game_version.as_deref(),
            config_dir: &self.paths.config_dir,
            fetcher,
        }
    }
}

struct Selected {
    key: String,
    install: GameInstall,
    user_added: bool,
}

pub struct Shell {
    deps: Deps,
    catalog: RwLock<Option<Arc<CatalogFetch>>>,
    /// Install key -> `--version` result. Cleared by `list_installs(refresh = true)`.
    versions: Mutex<HashMap<String, Option<String>>>,
    pending: Mutex<Option<Pending>>,
    planning: Mutex<Option<Ticket>>,
    next_id: AtomicU32,
    /// Serializes everything that writes the manager's or the game's files.
    op_lock: Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while holding one of these can't leave the data half-updated in a way that
    // matters (every guarded value is replaced wholesale), so recover rather than poison
    // every later command.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn empty_catalog() -> Arc<CatalogFetch> {
    Arc::new(CatalogFetch {
        entries: Vec::new(),
        source: FetchSource::NotModified,
        fetched_at: std::time::UNIX_EPOCH,
    })
}

impl Shell {
    pub fn new(deps: Deps) -> Self {
        Shell {
            deps,
            catalog: RwLock::new(None),
            versions: Mutex::new(HashMap::new()),
            pending: Mutex::new(None),
            planning: Mutex::new(None),
            next_id: AtomicU32::new(1),
            op_lock: Mutex::new(()),
        }
    }

    // -----------------------------------------------------------------------
    // Catalog
    // -----------------------------------------------------------------------

    fn catalog_cache_dir(&self) -> PathBuf {
        self.deps.cache_dir.join("catalog")
    }

    fn loaded_catalog(&self) -> Option<Arc<CatalogFetch>> {
        self.catalog
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// The in-memory catalog, fetching (through the ETag cache) when there is none yet or
    /// when `refresh` is asked for.
    pub fn load_catalog(&self, refresh: bool) -> CmdResult<CatalogView> {
        if !refresh && let Some(fetch) = self.loaded_catalog() {
            return Ok(CatalogView::from(fetch.as_ref()));
        }
        let cache_dir = self.catalog_cache_dir();
        let fetch = (self.deps.fetch_catalog)(&cache_dir)?;
        if fetch.source == FetchSource::Fresh {
            let urls: Vec<&str> = fetch
                .entries
                .iter()
                .filter_map(|e| e.icon_url.as_deref())
                .collect();
            // Best-effort housekeeping; a failure here never fails the catalog load.
            let _ = catalog::prune_icons(&cache_dir, &urls);
        }
        let view = CatalogView::from(&fetch);
        *self.catalog.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(fetch));
        Ok(view)
    }

    /// A catalog icon as a `data:` URL. Only URLs the current catalog actually lists are
    /// fetched, so the webview can't use this as a general-purpose download proxy.
    pub fn icon(&self, url: &str) -> CmdResult<String> {
        let listed = self
            .loaded_catalog()
            .is_some_and(|c| c.entries.iter().any(|e| e.icon_url.as_deref() == Some(url)));
        if !listed {
            return Err(CmdError::not_found(
                "That icon isn't in the loaded catalog.",
            ));
        }
        let path = (self.deps.fetch_icon)(&self.catalog_cache_dir(), url)?;
        let bytes = std::fs::read(&path)?;
        let mime = match path.extension().and_then(|e| e.to_str()) {
            Some("jpg" | "jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            _ => "image/png",
        };
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        Ok(format!("data:{mime};base64,{b64}"))
    }

    // -----------------------------------------------------------------------
    // Game installs
    // -----------------------------------------------------------------------

    fn all_installs(&self, settings: &Settings) -> Vec<(String, GameInstall, bool)> {
        let mut out: Vec<(String, GameInstall, bool)> = Vec::new();
        let detected = self
            .deps
            .env
            .as_ref()
            .map(game_install::detect)
            .unwrap_or_default();
        let custom = settings
            .custom
            .iter()
            .filter_map(|c| c.to_install(self.deps.env.as_ref()));
        for (install, user_added) in detected
            .into_iter()
            .map(|i| (i, false))
            .chain(custom.map(|i| (i, true)))
        {
            let key = install_key(&install);
            if !out.iter().any(|(k, _, _)| *k == key) {
                out.push((key, install, user_added));
            }
        }
        out
    }

    fn selected(&self) -> Option<Selected> {
        let settings = Settings::load(&self.deps.data_dir);
        let installs = self.all_installs(&settings);
        let chosen = settings
            .selected
            .as_ref()
            .and_then(|key| installs.iter().find(|(k, _, _)| k == key))
            .or_else(|| installs.first())?;
        let (key, install, user_added) = chosen.clone();
        Some(Selected {
            key,
            install,
            user_added,
        })
    }

    fn game_version(&self, key: &str, install: &GameInstall) -> Option<String> {
        if let Some(cached) = lock(&self.versions).get(key) {
            return cached.clone();
        }
        let version = (self.deps.query_version)(install);
        lock(&self.versions).insert(key.to_string(), version.clone());
        version
    }

    fn install_view(&self, key: &str, install: &GameInstall, user_added: bool) -> InstallView {
        let version = self.game_version(key, install);
        InstallView::new(key.to_string(), install, user_added, version)
    }

    pub fn list_installs(&self, refresh: bool) -> InstallsView {
        if refresh {
            lock(&self.versions).clear();
        }
        let settings = Settings::load(&self.deps.data_dir);
        let installs = self.all_installs(&settings);
        let selected = self.selected().map(|s| s.key);
        InstallsView {
            installs: installs
                .iter()
                .map(|(key, install, user_added)| self.install_view(key, install, *user_added))
                .collect(),
            selected,
        }
    }

    /// Changing installs abandons any plan built against the old one.
    fn reset_plans(&self) {
        self.cancel_planning();
        lock(&self.pending).take();
    }

    pub fn select_install(&self, key: &str) -> CmdResult<InstallsView> {
        let _op = lock(&self.op_lock);
        let mut settings = Settings::load(&self.deps.data_dir);
        if !self
            .all_installs(&settings)
            .iter()
            .any(|(k, _, _)| k == key)
        {
            return Err(CmdError::not_found("That install is no longer available."));
        }
        settings.selected = Some(key.to_string());
        settings.save(&self.deps.data_dir)?;
        self.reset_plans();
        drop(_op);
        Ok(self.list_installs(false))
    }

    /// Adds and selects a user-specified install. `config_dir` alone is a custom `-c` config
    /// dir; `executable` alone is an install using the default config dir (e.g. an AppImage).
    pub fn add_custom_install(
        &self,
        config_dir: Option<String>,
        executable: Option<String>,
    ) -> CmdResult<InstallsView> {
        let config_dir = config_dir
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let executable = executable
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if config_dir.is_none() && executable.is_none() {
            return Err(CmdError::invalid(
                "Give a config folder, a game executable, or both.",
            ));
        }
        if let Some(dir) = &config_dir
            && !Path::new(dir).is_absolute()
        {
            return Err(CmdError::invalid(
                "The config folder must be an absolute path.",
            ));
        }
        if let Some(exe) = &executable
            && !Path::new(exe).is_file()
        {
            return Err(CmdError::invalid(format!("No file at {exe:?}.")));
        }
        let custom = CustomInstall {
            config_dir: config_dir.map(PathBuf::from),
            executable: executable.map(PathBuf::from),
        };
        let install = custom.to_install(self.deps.env.as_ref()).ok_or_else(|| {
            CmdError::invalid("Can't find the default config folder; give one explicitly.")
        })?;
        let key = install_key(&install);

        let _op = lock(&self.op_lock);
        let mut settings = Settings::load(&self.deps.data_dir);
        if !settings.custom.contains(&custom) {
            settings.custom.push(custom);
        }
        settings.selected = Some(key);
        settings.save(&self.deps.data_dir)?;
        self.reset_plans();
        drop(_op);
        Ok(self.list_installs(false))
    }

    pub fn remove_custom_install(&self, key: &str) -> CmdResult<InstallsView> {
        let _op = lock(&self.op_lock);
        let mut settings = Settings::load(&self.deps.data_dir);
        let env = self.deps.env.as_ref();
        let before = settings.custom.len();
        settings.custom.retain(|c| {
            c.to_install(env)
                .is_none_or(|install| install_key(&install) != key)
        });
        if settings.custom.len() == before {
            return Err(CmdError::not_found("No user-added install with that key."));
        }
        if settings.selected.as_deref() == Some(key) {
            settings.selected = None;
            self.reset_plans();
        }
        settings.save(&self.deps.data_dir)?;
        drop(_op);
        Ok(self.list_installs(false))
    }

    pub fn game_status(&self) -> GameProcessView {
        (self.deps.detect_game)().into()
    }

    pub fn launch_game(&self) -> CmdResult<()> {
        let selected = self.selected().ok_or_else(CmdError::no_install)?;
        if selected.install.launch == Launch::Unknown {
            return Err(CmdError::invalid(
                "No known way to start this install. Add its executable in Settings.",
            ));
        }
        (self.deps.launch)(&selected.install)
            .map_err(|e| CmdError::io(format!("Couldn't start Endless Sky: {e}")))
    }

    // -----------------------------------------------------------------------
    // State
    // -----------------------------------------------------------------------

    fn snapshot(&self) -> CmdResult<(Selected, Snapshot)> {
        let selected = self.selected().ok_or_else(CmdError::no_install)?;
        let paths = InstallPaths::new(&self.deps.data_dir, &selected.install.config_dir);
        let records = records::load(&paths.records)?;
        let installed = manager::scan_installed(
            &install::plugins_dir(&selected.install.config_dir),
            &records,
        )?;
        let states = game_state::read_plugin_states(&selected.install.config_dir)?;
        let game_version = self.game_version(&selected.key, &selected.install);
        let snapshot = Snapshot {
            paths,
            records,
            installed,
            states,
            catalog: self.loaded_catalog().unwrap_or_else(empty_catalog),
            game_version,
        };
        Ok((selected, snapshot))
    }

    /// Everything the main views show, read fresh. Also performs decision B's automatic
    /// adoption (once the catalog is loaded) and decision E's first-run "Default" profile,
    /// since both are defined as happening on their own as soon as the state is seen.
    pub fn get_state(&self) -> CmdResult<ManagerState> {
        let _op = lock(&self.op_lock);
        let game = (self.deps.detect_game)().into();
        let Some(selected) = self.selected() else {
            return Ok(ManagerState {
                install: None,
                game,
                plugins: Vec::new(),
                unmanaged: Vec::new(),
                profiles: ProfilesView::default(),
                adopted: Vec::new(),
                catalog_loaded: self.loaded_catalog().is_some(),
            });
        };
        let (_, mut snap) = self.snapshot()?;
        snap.paths.write_marker();
        let catalog = self.loaded_catalog();
        let plugins_dir = install::plugins_dir(&snap.paths.config_dir);

        let mut adopted = Vec::new();
        let mut unmanaged = Vec::new();
        if let Some(catalog) = &catalog {
            let result =
                manager::adopt_candidates(&snap.installed, &catalog.entries, &snap.records);
            if !result.adopted.is_empty() {
                for record in result.adopted {
                    adopted.push(record.folder.clone());
                    snap.records.insert(record.folder.clone(), record);
                }
                records::save(&snap.paths.records, &snap.records)?;
                snap.installed = manager::scan_installed(&plugins_dir, &snap.records)?;
            }
            unmanaged = result
                .needs_user
                .into_iter()
                .map(|u| UnmanagedView {
                    folder: u.folder,
                    identity: u.identity,
                    candidates: u.candidates.into_iter().map(|c| c.name).collect(),
                })
                .collect();
        }

        let identities = snap.identities();
        let mut store = ProfileStore::load(&snap.paths.profiles)?;
        if store.ensure_default(&identities, &snap.states, &snap.records) {
            store.save(&snap.paths.profiles)?;
        }
        let profiles = profiles_view(&store, &identities, &snap.states);

        let plugins = snap
            .installed
            .iter()
            .map(|p| {
                let update = match (&p.record, &catalog) {
                    (None, _) => UpdateView::Unmanaged,
                    (Some(_), None) => UpdateView::Unchecked,
                    (Some(record), Some(catalog)) => {
                        manager::update_status(record, &catalog.entries).into()
                    }
                };
                let enabled = game_state::effective_enabled(&snap.states, &p.identity);
                InstalledPluginView::new(p, enabled, update)
            })
            .collect();

        Ok(ManagerState {
            install: Some(self.install_view(&selected.key, &selected.install, selected.user_added)),
            game,
            plugins,
            unmanaged,
            profiles,
            adopted,
            catalog_loaded: catalog.is_some(),
        })
    }

    /// Decision B's "ask the user": links an unmanaged plugin to the catalog entry they
    /// picked. Like automatic adoption, the version is left unknown.
    pub fn adopt_plugin(&self, folder: &str, catalog_name: &str) -> CmdResult<()> {
        let _op = lock(&self.op_lock);
        let (_, snap) = self.snapshot()?;
        let plugin = snap
            .installed
            .iter()
            .find(|p| p.folder == folder)
            .ok_or_else(|| CmdError::not_found(format!("{folder:?} isn't installed.")))?;
        if plugin.record.is_some() {
            return Err(CmdError::invalid(format!("{folder:?} is already managed.")));
        }
        let entry = find_entry(&snap.catalog.entries, catalog_name)?;
        let mut records = snap.records.clone();
        records.insert(
            folder.to_string(),
            InstallRecord {
                catalog_name: entry.name.clone(),
                folder: folder.to_string(),
                identity: plugin.identity.clone(),
                version: String::new(),
                source_url: entry.url.clone(),
                sha256: String::new(),
            },
        );
        records::save(&snap.paths.records, &records)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Planning
    // -----------------------------------------------------------------------

    /// Starts a planning run, superseding (and cancelling) any earlier one and discarding
    /// the pending plan: only one plan is ever offered at a time.
    pub fn begin_planning(&self) -> Ticket {
        let ticket = Ticket {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        if let Some(old) = lock(&self.planning).replace(ticket.clone()) {
            old.cancel.store(true, Ordering::Relaxed);
        }
        lock(&self.pending).take();
        ticket
    }

    /// Cancels the in-flight planning run, if any. Its download stops at the next read (or,
    /// if stalled inside one, is abandoned by `commands.rs`), and its result is discarded.
    pub fn cancel_planning(&self) {
        if let Some(ticket) = lock(&self.planning).take() {
            ticket.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Stores a finished plan as the pending one, unless its run was cancelled or
    /// superseded meanwhile (then the plan is dropped, which deletes its staged downloads).
    fn finish(&self, ticket: &Ticket, pending: Pending) -> CmdResult<PlanView> {
        let mut planning = lock(&self.planning);
        let current = planning.as_ref().is_some_and(|t| t.id == ticket.id);
        if !current || ticket.cancel.load(Ordering::Relaxed) {
            return Err(CmdError::cancelled());
        }
        planning.take();
        let view = pending.view();
        *lock(&self.pending) = Some(pending);
        Ok(view)
    }

    fn check_live(ticket: &Ticket) -> CmdResult<()> {
        if ticket.cancel.load(Ordering::Relaxed) {
            Err(CmdError::cancelled())
        } else {
            Ok(())
        }
    }

    fn plan_fetcher(&self, ticket: &Ticket) -> PlanFetcher {
        PlanFetcher {
            inner: self.deps.fetcher.clone(),
            plan_id: ticket.id,
            cancel: ticket.cancel.clone(),
            sink: self.deps.progress.clone(),
        }
    }

    fn require_catalog(snap: &Snapshot) -> CmdResult<()> {
        if snap.catalog.entries.is_empty() {
            return Err(CmdError::Network {
                message: "The plugin catalog isn't loaded yet.".into(),
            });
        }
        Ok(())
    }

    pub fn plan_install(&self, ticket: &Ticket, catalog_name: &str) -> CmdResult<PlanView> {
        let (_, snap) = self.snapshot()?;
        Self::require_catalog(&snap)?;
        let entry = find_entry(&snap.catalog.entries, catalog_name)?;
        if let Some(record) = snap
            .records
            .values()
            .find(|r| r.catalog_name == catalog_name)
        {
            return Err(CmdError::invalid(format!(
                "{catalog_name} is already installed (in {:?}); update it instead.",
                record.folder
            )));
        }
        Self::check_live(ticket)?;
        let fetcher = self.plan_fetcher(ticket);
        let plan = manager::plan_install(entry, &snap.plan_ctx(&fetcher));
        Self::check_live(ticket)?;
        // The plugin asked for couldn't even be downloaded: there's nothing to review or
        // override, just an error to show (and retry).
        if !plan
            .steps
            .iter()
            .any(|s| matches!(s, PlanStep::Install(staged) if staged.catalog_name == catalog_name))
        {
            let error = plan
                .issues
                .iter()
                .find_map(|issue| match issue {
                    Issue::CatalogDownloadFailed {
                        catalog_name: name,
                        error,
                    } if name == catalog_name => Some(error.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "unknown error".into());
            return Err(CmdError::Network {
                message: format!("Couldn't download {catalog_name}: {error}"),
            });
        }
        let root_identity = plan.steps.iter().rev().find_map(|s| match s {
            PlanStep::Install(staged) if staged.catalog_name == catalog_name => {
                Some(staged.identity.clone())
            }
            _ => None,
        });
        let fixes = requirement_fixes(&plan.issues, &snap);
        self.finish(
            ticket,
            Pending {
                id: ticket.id,
                paths: snap.paths.clone(),
                target: catalog_name.to_string(),
                plan: PendingPlan::Install {
                    plan,
                    root_catalog_name: catalog_name.to_string(),
                },
                fixes,
                missing: Vec::new(),
                game_version: snap.game_version.clone(),
                unmanaged_target: false,
                root_identity,
            },
        )
    }

    pub fn plan_update(&self, ticket: &Ticket, folder: &str) -> CmdResult<PlanView> {
        let (_, snap) = self.snapshot()?;
        Self::require_catalog(&snap)?;
        let current = find_installed(&snap, folder)?;
        let record = current.record.as_ref().ok_or_else(|| {
            CmdError::invalid(format!(
                "{folder:?} isn't managed; link it to a catalog entry first."
            ))
        })?;
        let entry = find_entry(&snap.catalog.entries, &record.catalog_name)?;
        let step = StepView::Update {
            catalog_name: entry.name.clone(),
            identity: current.identity.clone(),
            folder: folder.to_string(),
            from: record.version.clone(),
            to: entry.version.clone(),
        };
        Self::check_live(ticket)?;
        let fetcher = self.plan_fetcher(ticket);
        let plan = manager::plan_update(folder, &snap.plan_ctx(&fetcher)).map_err(|e| {
            if ticket.cancel.load(Ordering::Relaxed) {
                CmdError::cancelled()
            } else {
                CmdError::Network { message: e }
            }
        })?;
        let fixes = requirement_fixes(&plan.issues, &snap);
        self.finish(
            ticket,
            Pending {
                id: ticket.id,
                paths: snap.paths.clone(),
                target: folder.to_string(),
                plan: PendingPlan::Update { plan, step },
                fixes,
                missing: Vec::new(),
                game_version: snap.game_version.clone(),
                unmanaged_target: false,
                root_identity: None,
            },
        )
    }

    /// One combined plan over every plugin with `UpdateView::Available` (the "update all"
    /// button): each is planned the same way `plan_update` plans one, but all of their
    /// issues/notes/steps are combined into a single review before anything commits, rather
    /// than one plan/commit dialog per plugin. A plugin whose download fails is skipped
    /// (not staged), with a `CatalogDownloadFailed` issue reported for it like
    /// `plan_install`'s recursive requirement-fetching already does - that still blocks the
    /// whole batch until overridden, the same way any other issue does.
    pub fn plan_update_all(&self, ticket: &Ticket) -> CmdResult<PlanView> {
        let (_, snap) = self.snapshot()?;
        Self::require_catalog(&snap)?;
        let outdated: Vec<String> = snap
            .installed
            .iter()
            .filter(|p| {
                p.record.as_ref().is_some_and(|r| {
                    matches!(
                        manager::update_status(r, &snap.catalog.entries),
                        manager::UpdateStatus::Available { .. }
                    )
                })
            })
            .map(|p| p.folder.clone())
            .collect();
        if outdated.is_empty() {
            return Err(CmdError::invalid("No updates are available."));
        }

        let fetcher = self.plan_fetcher(ticket);
        let mut plans = Vec::new();
        let mut steps = Vec::new();
        let mut issues = Vec::new();
        let mut notes = Vec::new();
        for folder in &outdated {
            Self::check_live(ticket)?;
            let current = find_installed(&snap, folder)?;
            // Safe: `outdated` only contains folders whose `record` was `Some` above.
            let record = current.record.as_ref().expect("checked above");
            let entry = find_entry(&snap.catalog.entries, &record.catalog_name)?;
            match manager::plan_update(folder, &snap.plan_ctx(&fetcher)) {
                Ok(plan) => {
                    steps.extend(requirement_steps(&plan));
                    steps.push(StepView::Update {
                        catalog_name: entry.name.clone(),
                        identity: current.identity.clone(),
                        folder: folder.clone(),
                        from: record.version.clone(),
                        to: entry.version.clone(),
                    });
                    issues.extend(plan.issues.iter().cloned());
                    notes.extend(plan.notes.iter().cloned());
                    plans.push(plan);
                }
                Err(error) => {
                    if ticket.cancel.load(Ordering::Relaxed) {
                        return Err(CmdError::cancelled());
                    }
                    issues.push(Issue::CatalogDownloadFailed {
                        catalog_name: entry.name.clone(),
                        error,
                    });
                }
            }
        }
        let fixes = requirement_fixes(&issues, &snap);
        self.finish(
            ticket,
            Pending {
                id: ticket.id,
                paths: snap.paths.clone(),
                target: format!(
                    "{} plugin{}",
                    outdated.len(),
                    if outdated.len() == 1 { "" } else { "s" }
                ),
                plan: PendingPlan::UpdateAll {
                    plans,
                    steps,
                    issues,
                    notes,
                },
                fixes,
                missing: Vec::new(),
                game_version: snap.game_version.clone(),
                unmanaged_target: false,
                root_identity: None,
            },
        )
    }

    pub fn plan_enable(&self, ticket: &Ticket, identity: &str) -> CmdResult<PlanView> {
        let (_, snap) = self.snapshot()?;
        find_identity(&snap, identity)?;
        let fetcher = self.plan_fetcher(ticket);
        let plan = manager::plan_enable(identity, &snap.plan_ctx(&fetcher));
        let fixes = requirement_fixes(&plan.issues, &snap);
        self.finish(
            ticket,
            simple_pending_with_root(
                ticket,
                &snap,
                identity,
                PendingPlan::Enable(plan),
                fixes,
                Some(identity.to_string()),
            ),
        )
    }

    pub fn plan_disable(&self, ticket: &Ticket, identity: &str) -> CmdResult<PlanView> {
        let (_, snap) = self.snapshot()?;
        find_identity(&snap, identity)?;
        let fetcher = self.plan_fetcher(ticket);
        let plan = manager::plan_disable(identity, &snap.plan_ctx(&fetcher));
        self.finish(
            ticket,
            simple_pending(
                ticket,
                &snap,
                identity,
                PendingPlan::Disable(plan),
                Vec::new(),
            ),
        )
    }

    pub fn plan_uninstall(&self, ticket: &Ticket, folder: &str) -> CmdResult<PlanView> {
        let (_, snap) = self.snapshot()?;
        let unmanaged = find_installed(&snap, folder)?.record.is_none();
        let fetcher = self.plan_fetcher(ticket);
        let plan = manager::plan_uninstall(folder, &snap.plan_ctx(&fetcher));
        let mut pending = simple_pending(
            ticket,
            &snap,
            folder,
            PendingPlan::Uninstall(plan),
            Vec::new(),
        );
        pending.unmanaged_target = unmanaged;
        self.finish(ticket, pending)
    }

    /// Switching to (or restoring) a profile, as a plan: the same decision C checks as any
    /// other change run over the state the profile would produce, so switching into a
    /// profile with a conflict or an unmet `requires` is shown before anything is written.
    pub fn plan_apply_profile(&self, ticket: &Ticket, name: &str) -> CmdResult<PlanView> {
        let (_, snap) = self.snapshot()?;
        let store = ProfileStore::load(&snap.paths.profiles)?;
        let profile = store
            .profiles
            .get(name)
            .ok_or_else(|| CmdError::not_found(format!("No profile named {name:?}.")))?;
        let identities = snap.identities();
        let (states, missing) = profiles::apply(profile, &identities, &snap.states);
        let (steps, issues, notes) = apply_profile_fields(&snap, &states);
        let fixes = requirement_fixes(&issues, &snap);
        let mut pending = simple_pending(
            ticket,
            &snap,
            name,
            PendingPlan::ApplyProfile {
                name: name.to_string(),
                states,
                steps,
                issues,
                notes,
            },
            fixes,
        );
        pending.missing = missing.iter().map(MissingView::from).collect();
        self.finish(ticket, pending)
    }

    /// Resolves a `Conflict` issue naming `identity` by disabling it as part of the same
    /// commit, instead of requiring an override: `InstallPlan`/`EnablePlan::disable_to_resolve`
    /// for those plan kinds, or (`ApplyProfile` has no such method, since it isn't a core-owned
    /// struct) flipping `identity`'s bit in the plan's own `states` directly and recomputing
    /// `steps`/`issues`/`notes` from it the same way `plan_apply_profile` does initially.
    pub fn resolve_conflict(&self, plan_id: u32, identity: &str) -> CmdResult<PlanView> {
        let mut slot = lock(&self.pending);
        let pending = slot
            .as_mut()
            .filter(|p| p.id == plan_id)
            .ok_or_else(CmdError::plan_expired)?;
        if !pending.resolvable_conflicts().iter().any(|i| i == identity) {
            return Err(CmdError::invalid(format!(
                "Disabling {identity:?} doesn't resolve a conflict in this plan."
            )));
        }
        match &mut pending.plan {
            PendingPlan::Install { plan, .. } => plan.disable_to_resolve(identity),
            PendingPlan::Enable(plan) => plan.disable_to_resolve(identity),
            PendingPlan::ApplyProfile {
                states,
                steps,
                issues,
                notes,
                ..
            } => {
                let (_, snap) = self.snapshot()?;
                states.insert(identity.to_string(), false);
                (*steps, *issues, *notes) = apply_profile_fields(&snap, states);
            }
            _ => {}
        }
        Ok(pending.view())
    }

    pub fn discard_plan(&self, plan_id: u32) {
        let mut slot = lock(&self.pending);
        if slot.as_ref().is_some_and(|p| p.id == plan_id) {
            slot.take();
        }
    }

    // -----------------------------------------------------------------------
    // Commit
    // -----------------------------------------------------------------------

    /// Commits the pending plan. Refusals that the user can act on (blocking issues without
    /// an override, the game running) leave the plan pending; anything that actually
    /// attempted the commit consumes it.
    pub fn commit_plan(&self, plan_id: u32, override_issues: bool) -> CmdResult<CommitView> {
        let _op = lock(&self.op_lock);
        let pending = {
            let mut slot = lock(&self.pending);
            let pending = slot
                .as_ref()
                .filter(|p| p.id == plan_id)
                .ok_or_else(CmdError::plan_expired)?;
            if !override_issues && !pending.issues().is_empty() {
                return Err(CmdError::blocked(pending.issues()));
            }
            if (self.deps.detect_game)() == GameProcess::Running {
                return Err(CmdError::game_running());
            }
            slot.take().expect("checked above")
        };

        let paths = &pending.paths;
        let ctx = CommitContext {
            detect_game: self.deps.detect_game,
            ..CommitContext::new(&paths.config_dir, &paths.records, &paths.profiles)
        };
        let report = match pending.plan {
            PendingPlan::Install { plan, .. } => manager::commit(plan, &ctx, override_issues)?,
            PendingPlan::Update { plan, .. } => {
                manager::commit_update(plan, &ctx, override_issues)?
            }
            PendingPlan::UpdateAll { plans, .. } => {
                manager::commit_update_all(plans, &ctx, override_issues)?
            }
            PendingPlan::Enable(plan) => manager::commit_enable(plan, &ctx, override_issues)?,
            PendingPlan::Disable(plan) => manager::commit_disable(plan, &ctx, override_issues)?,
            PendingPlan::Uninstall(plan) => manager::commit_uninstall(plan, &ctx, override_issues)?,
            PendingPlan::ApplyProfile {
                name,
                states,
                steps,
                ..
            } => return self.commit_profile(paths, &name, &states, &steps),
        };
        Ok(CommitView::from(&report))
    }

    fn commit_profile(
        &self,
        paths: &InstallPaths,
        name: &str,
        states: &PluginStates,
        steps: &[StepView],
    ) -> CmdResult<CommitView> {
        let game = (self.deps.detect_game)();
        let mut store = ProfileStore::load(&paths.profiles)?;
        store.set_active(name)?;
        let outcome = game_state::write_plugin_states(&paths.config_dir, states, game)?;
        store.save(&paths.profiles)?;
        let mut view = CommitView {
            backup: outcome.backup.map(|p| p.display().to_string()),
            game_detection_failed: outcome.detection_failed,
            ..CommitView::default()
        };
        for step in steps {
            match step {
                StepView::Enable { identity } => view.enabled.push(identity.clone()),
                StepView::Disable { identity } => view.disabled.push(identity.clone()),
                _ => {}
            }
        }
        Ok(view)
    }

    // -----------------------------------------------------------------------
    // Profiles (the changes that don't touch plugins.txt need no plan)
    // -----------------------------------------------------------------------

    fn with_profiles<T>(
        &self,
        f: impl FnOnce(&mut ProfileStore, &Snapshot) -> CmdResult<T>,
    ) -> CmdResult<T> {
        let _op = lock(&self.op_lock);
        let (_, snap) = self.snapshot()?;
        let mut store = ProfileStore::load(&snap.paths.profiles)?;
        let out = f(&mut store, &snap)?;
        store.save(&snap.paths.profiles)?;
        Ok(out)
    }

    /// Saves the current enabled set as a new profile and makes it active (also decision E's
    /// "save the current state as a new named profile" drift resolution). Returns the
    /// stored (trimmed) name.
    pub fn create_profile(&self, name: &str) -> CmdResult<String> {
        self.with_profiles(|store, snap| {
            Ok(store.save_current_as(name, &snap.identities(), &snap.states, &snap.records)?)
        })
    }

    /// Returns the stored (trimmed) name, same as `create_profile`.
    pub fn rename_profile(&self, old_name: &str, new_name: &str) -> CmdResult<String> {
        self.with_profiles(|store, _| Ok(store.rename(old_name, new_name)?))
    }

    /// Drift resolution: make the active profile match the live `plugins.txt`.
    pub fn update_active_profile(&self) -> CmdResult<()> {
        self.with_profiles(|store, snap| {
            Ok(store.update_active(&snap.identities(), &snap.states, &snap.records)?)
        })
    }

    /// Writes `name` to `path` as a share file (see `profiles::export_profile`).
    pub fn export_profile(&self, name: &str, path: &Path) -> CmdResult<()> {
        let _op = lock(&self.op_lock);
        let (_, snap) = self.snapshot()?;
        let store = ProfileStore::load(&snap.paths.profiles)?;
        let profile = store
            .profiles
            .get(name)
            .ok_or_else(|| CmdError::not_found(format!("No profile named {name:?}.")))?;
        let text = profiles::export_profile(name, profile)?;
        std::fs::write(path, text)
            .map_err(|e| CmdError::io(format!("Couldn't save the profile file: {e}")))
    }

    /// Reads a share file from `path` into a new profile (not activated: switching to it goes
    /// through the normal checked plan). Returns the stored name, numbered if it was taken.
    pub fn import_profile(&self, path: &Path) -> CmdResult<String> {
        let size = std::fs::metadata(path)
            .map_err(|e| CmdError::io(format!("Couldn't read that file: {e}")))?
            .len();
        if size > profiles::MAX_SHARE_BYTES {
            return Err(CmdError::invalid("That file is too large to be a profile."));
        }
        let text = std::fs::read_to_string(path).map_err(|_| {
            CmdError::invalid("That isn't a profile file made by Endless Sky Mod Manager.")
        })?;
        let (name, profile) = profiles::parse_shared_profile(&text)?;
        self.with_profiles(|store, _| Ok(store.insert_unique(&name, profile)?))
    }

    pub fn delete_profile(&self, name: &str) -> CmdResult<()> {
        self.with_profiles(|store, _| {
            if store.active.as_deref() == Some(name) {
                return Err(CmdError::invalid(
                    "Can't delete the active profile. Switch to another one first.",
                ));
            }
            store
                .profiles
                .remove(name)
                .map(|_| ())
                .ok_or_else(|| CmdError::not_found(format!("No profile named {name:?}.")))
        })
    }
}

fn simple_pending(
    ticket: &Ticket,
    snap: &Snapshot,
    target: &str,
    plan: PendingPlan,
    fixes: Vec<RequirementFixView>,
) -> Pending {
    simple_pending_with_root(ticket, snap, target, plan, fixes, None)
}

/// Like [`simple_pending`], but with a `root_identity` to exclude from
/// `Pending::resolvable_conflicts` (the identity the plan is already about -- disabling it
/// "to resolve" its own conflict would be the same as not enabling it at all).
fn simple_pending_with_root(
    ticket: &Ticket,
    snap: &Snapshot,
    target: &str,
    plan: PendingPlan,
    fixes: Vec<RequirementFixView>,
    root_identity: Option<String>,
) -> Pending {
    Pending {
        id: ticket.id,
        paths: snap.paths.clone(),
        target: target.to_string(),
        plan,
        fixes,
        missing: Vec::new(),
        game_version: snap.game_version.clone(),
        unmanaged_target: false,
        root_identity,
    }
}

fn find_entry<'a>(catalog: &'a [CatalogEntry], name: &str) -> CmdResult<&'a CatalogEntry> {
    catalog
        .iter()
        .find(|e| e.name == name)
        .ok_or_else(|| CmdError::not_found(format!("{name:?} isn't in the catalog.")))
}

fn find_installed<'a>(snap: &'a Snapshot, folder: &str) -> CmdResult<&'a InstalledPlugin> {
    snap.installed
        .iter()
        .find(|p| p.folder == folder)
        .ok_or_else(|| CmdError::not_found(format!("{folder:?} isn't installed.")))
}

fn find_identity<'a>(snap: &'a Snapshot, identity: &str) -> CmdResult<&'a InstalledPlugin> {
    snap.installed
        .iter()
        .find(|p| p.identity == identity)
        .ok_or_else(|| CmdError::not_found(format!("No installed plugin is named {identity:?}.")))
}

/// Review steps for what an update installs/enables beyond the updated plugin itself (a
/// requirement its new version added), marked as dependencies like an install plan's.
fn requirement_steps(plan: &UpdatePlan) -> Vec<StepView> {
    plan.new_requirements()
        .iter()
        .map(|step| match step {
            PlanStep::Install(staged) => StepView::Install {
                catalog_name: staged.catalog_name.clone(),
                identity: staged.identity.clone(),
                folder: staged.folder.clone(),
                version: staged.version.clone(),
                dependency: true,
            },
            PlanStep::Enable(identity) => StepView::Enable {
                identity: identity.clone(),
            },
            PlanStep::Disable(identity) => StepView::Disable {
                identity: identity.clone(),
            },
        })
        .collect()
}

/// `steps`/`issues`/`notes` for an `ApplyProfile` plan's `states`, against `snap`'s current
/// live state. Shared by `plan_apply_profile` (building the plan from scratch) and
/// `resolve_conflict` (recomputing it after flipping one identity's bit to resolve a
/// conflict), so the two can never drift apart on how a profile's effect is described.
fn apply_profile_fields(
    snap: &Snapshot,
    states: &PluginStates,
) -> (Vec<StepView>, Vec<Issue>, Vec<Note>) {
    let planned: Vec<PlannedPlugin> = snap
        .installed
        .iter()
        .map(|p| PlannedPlugin {
            identity: &p.identity,
            enabled: game_state::effective_enabled(states, &p.identity),
            meta: &p.meta,
        })
        .collect();
    let mut issues = resolve::check_state(&planned, snap.game_version.as_deref());
    issues.extend(resolve::check_requirements(
        &planned,
        &snap.catalog.entries,
        &snap.records,
    ));
    let notes = resolve::optional_notes(&planned);
    let steps = snap
        .installed
        .iter()
        .filter_map(|p| {
            let before = game_state::effective_enabled(&snap.states, &p.identity);
            let after = game_state::effective_enabled(states, &p.identity);
            match (before, after) {
                (false, true) => Some(StepView::Enable {
                    identity: p.identity.clone(),
                }),
                (true, false) => Some(StepView::Disable {
                    identity: p.identity.clone(),
                }),
                _ => None,
            }
        })
        .collect();
    (steps, issues, notes)
}

/// For each unmet requirement, the action that would satisfy it: enabling it when it's
/// installed but disabled, else installing its catalog match (or picking among several).
///
/// This deliberately doesn't reuse `EnablePlan::requirement_fixes`, which only ever offers a
/// catalog install: `check_requirements` treats an installed-but-disabled requirement as
/// missing, and reinstalling it would be the wrong fix.
fn requirement_fixes(issues: &[Issue], snap: &Snapshot) -> Vec<RequirementFixView> {
    let mut seen = BTreeSet::new();
    let mut fixes = Vec::new();
    for issue in issues {
        let requires = match issue {
            Issue::MissingRequirement { requires, .. }
            | Issue::AmbiguousRequirement { requires, .. } => requires,
            _ => continue,
        };
        if !seen.insert(requires.clone()) {
            continue;
        }
        let mut fix = RequirementFixView {
            requires: requires.clone(),
            enable: None,
            install: None,
            candidates: Vec::new(),
        };
        if snap.installed.iter().any(|p| &p.identity == requires) {
            fix.enable = Some(requires.clone());
        } else {
            match resolve::match_catalog(requires, &snap.catalog.entries, &snap.records) {
                CatalogMatch::Exact(e) | CatalogMatch::Likely(e) => {
                    fix.install = Some(e.name.clone())
                }
                CatalogMatch::Ambiguous(candidates) => {
                    fix.candidates = candidates.into_iter().map(|e| e.name.clone()).collect()
                }
                CatalogMatch::NoMatch => continue,
            }
        }
        fixes.push(fix);
    }
    fixes
}

fn profiles_view(
    store: &ProfileStore,
    identities: &BTreeSet<String>,
    states: &PluginStates,
) -> ProfilesView {
    let (drift, missing) = match store.active_profile() {
        Some(profile) => (
            DriftView::from(&profiles::drift(profile, identities, states)),
            profiles::apply(profile, identities, states)
                .1
                .iter()
                .map(MissingView::from)
                .collect(),
        ),
        None => (DriftView::default(), Vec::new()),
    };
    ProfilesView {
        active: store.active.clone(),
        profiles: store
            .profiles
            .iter()
            .map(|(name, p)| ProfileSummary {
                name: name.clone(),
                enabled_count: p.enabled.len() as u32,
            })
            .collect(),
        drift,
        missing,
    }
}
