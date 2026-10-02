//! Orchestration with I/O: scanning installed plugins, planning an install/enable/disable/
//! uninstall/update against decision C's checks, and committing a plan atomically.
//!
//! Planning never touches `plugins/` or the game's `plugins.txt`; it only downloads into
//! staging under `<config>/.esmm-tmp/` (via the injected [`Fetcher`]) and reads the staged
//! `plugin.txt`. Nothing is installed, and no managed file is written, until [`commit`].

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use tempfile::TempDir;

use crate::catalog::CatalogEntry;
use crate::download::Downloaded;
use crate::game_state::{self, GameProcess};
use crate::install::{self, InstallError, Installed};
use crate::plugin_meta::PluginMeta;
use crate::plugin_state::PluginStates;
use crate::profiles::ProfileStore;
use crate::records::{self, InstallRecord, InstallRecords};
use crate::resolve::{self, CatalogMatch, Issue, Note, PlannedPlugin};

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct InstalledPlugin {
    pub folder: String,
    /// `plugin.txt` name, or the folder name when there is none or it has no `name`.
    pub identity: String,
    pub meta: PluginMeta,
    /// `None` for an unmanaged plugin (decision B): present in `plugins/` but not installed
    /// by us.
    pub record: Option<InstallRecord>,
}

/// Every plugin folder the game would load from `plugins_dir` (decision B's "unmanaged
/// plugins" included). Loose `.zip` files in `plugins/` are out of scope (the game can load
/// them directly, but this manager never produces or touches one) and are skipped, same as
/// any other non-plugin entry.
pub fn scan_installed(
    plugins_dir: &Path,
    records: &InstallRecords,
) -> io::Result<Vec<InstalledPlugin>> {
    let entries = match fs::read_dir(plugins_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() || !install::is_plugin(&path) {
            continue;
        }
        let folder = entry.file_name().to_string_lossy().into_owned();
        let meta = match fs::read_to_string(path.join("plugin.txt")) {
            Ok(text) => PluginMeta::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => PluginMeta::default(),
            Err(e) => return Err(e),
        };
        let identity = meta.identity(&folder).to_string();
        let record = records.get(&folder).cloned();
        found.push(InstalledPlugin {
            folder,
            identity,
            meta,
            record,
        });
    }
    found.sort_by(|a, b| a.folder.cmp(&b.folder));
    Ok(found)
}

// ---------------------------------------------------------------------------
// Fetcher injection
// ---------------------------------------------------------------------------

/// Downloads a catalog entry's current version to `dest`. Progress and cancel are threaded
/// straight through to the caller; a UI layer wanting live cancel wraps its own `Fetcher`
/// around a shared `AtomicBool` it owns.
pub trait Fetcher {
    fn fetch(
        &self,
        entry: &CatalogEntry,
        dest: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>),
        cancel: &AtomicBool,
    ) -> Result<Downloaded, String>;
}

/// The real `Fetcher`, over `download::download_to`.
pub struct HttpFetcher {
    pub max_bytes: u64,
}

impl Default for HttpFetcher {
    fn default() -> Self {
        // High DPI, the catalog's largest known plugin, is 794 MB (Mega Freight is 166 MB); the
        // cap only exists to stop a runaway or hostile link, so keep generous headroom.
        Self { max_bytes: 2 << 30 }
    }
}

impl Fetcher for HttpFetcher {
    fn fetch(
        &self,
        entry: &CatalogEntry,
        dest: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>),
        cancel: &AtomicBool,
    ) -> Result<Downloaded, String> {
        crate::download::download_to(&entry.url, dest, self.max_bytes, progress, cancel)
            .map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Planning context
// ---------------------------------------------------------------------------

/// Everything planning needs, all in memory or injected. Planning only performs downloads
/// (through `fetcher`) into `<config_dir>/.esmm-tmp/`; it never touches `plugins/` or
/// `plugins.txt`.
pub struct PlanContext<'a> {
    pub catalog: &'a [CatalogEntry],
    pub installed: &'a [InstalledPlugin],
    pub records: &'a InstallRecords,
    pub states: &'a PluginStates,
    /// The installed game's own version, when known (decision H / `query_install_version`).
    pub game_version: Option<&'a str>,
    pub config_dir: &'a Path,
    pub fetcher: &'a dyn Fetcher,
}

/// A plugin downloaded and extracted into staging, not yet installed.
#[derive(Debug)]
pub struct StagedPlugin {
    /// Deleted (and its contents with it) when dropped -- an uncommitted plan cleans up
    /// `.esmm-tmp` on its own. Never read; held purely for that `Drop`.
    #[allow(dead_code)]
    staging: TempDir,
    root: PathBuf,
    pub folder: String,
    pub catalog_name: String,
    pub identity: String,
    pub meta: PluginMeta,
    pub version: String,
    pub source_url: String,
    pub sha256: String,
}

fn stage_download(ctx: &PlanContext, entry: &CatalogEntry) -> Result<StagedPlugin, String> {
    let staging =
        install::new_staging_dir(&install::tmp_dir(ctx.config_dir)).map_err(|e| e.to_string())?;
    let zip_path = staging.path().join("download.zip");
    let downloaded =
        ctx.fetcher
            .fetch(entry, &zip_path, &mut |_, _| {}, &AtomicBool::new(false))?;
    let extract_dir = staging.path().join("extracted");
    install::extract_zip(&zip_path, &extract_dir, install::ExtractLimits::default())
        .map_err(|e| e.to_string())?;
    let root = install::find_plugin_root(&extract_dir).map_err(|e| e.to_string())?;
    let meta = match fs::read_to_string(root.join("plugin.txt")) {
        Ok(text) => PluginMeta::parse(&text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => PluginMeta::default(),
        Err(e) => return Err(e.to_string()),
    };
    let folder = install::sanitize_folder_name(&entry.name);
    let identity = meta.identity(&folder).to_string();
    Ok(StagedPlugin {
        staging,
        root,
        folder,
        catalog_name: entry.name.clone(),
        identity,
        meta,
        version: entry.version.clone(),
        source_url: entry.url.clone(),
        sha256: downloaded.sha256,
    })
}

// ---------------------------------------------------------------------------
// Install planning (recursive `requires` resolution)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum PlanStep {
    Install(Box<StagedPlugin>),
    /// An already-installed, currently-disabled dependency that must be enabled.
    Enable(String),
    /// Added by [`InstallPlan::disable_to_resolve`] to resolve a conflict.
    Disable(String),
}

#[derive(Debug)]
pub struct InstallPlan {
    /// Dependencies before dependents; diamond dependencies appear once.
    pub steps: Vec<PlanStep>,
    pub issues: Vec<Issue>,
    pub notes: Vec<Note>,
}

impl InstallPlan {
    /// Resolves a `Conflict` issue naming `identity` by disabling it instead of requiring an
    /// override. No-op if `identity` isn't actually in conflict in this plan.
    pub fn disable_to_resolve(&mut self, identity: &str) {
        let in_conflict = |issue: &Issue| matches!(issue, Issue::Conflict { a, b } if a == identity || b == identity);
        if !self.issues.iter().any(in_conflict) {
            return;
        }
        self.issues.retain(|issue| !in_conflict(issue));
        self.steps.push(PlanStep::Disable(identity.to_string()));
    }
}

/// Builds the ordered steps and any `requires`-resolution issues for a prospective install.
/// Conflict/game-version checks and optional-dependency notes are added once, over the
/// whole resulting state, by [`finalize_plan`].
struct PlanBuilder<'ctx, 'a> {
    ctx: &'ctx PlanContext<'a>,
    /// Identities already staged/queued in this plan, or already installed -- guards both
    /// diamond dependencies (staged once) and cycles (terminates: an ancestor is already
    /// "seen" by the time its own requirement loops back to it).
    seen_identities: HashSet<String>,
    seen_catalog_names: HashSet<String>,
    steps: Vec<PlanStep>,
    issues: Vec<Issue>,
}

impl<'ctx, 'a> PlanBuilder<'ctx, 'a> {
    fn new(ctx: &'ctx PlanContext<'a>) -> Self {
        Self {
            ctx,
            seen_identities: HashSet::new(),
            seen_catalog_names: HashSet::new(),
            steps: Vec::new(),
            issues: Vec::new(),
        }
    }

    fn stage_root(&mut self, entry: &CatalogEntry) {
        self.seen_catalog_names.insert(entry.name.clone());
        self.stage_and_walk(entry);
    }

    fn resolve_requirement(&mut self, required_identity: &str, requiring_plugin: &str) {
        if self.seen_identities.contains(required_identity) {
            return;
        }
        if let Some(installed) = self
            .ctx
            .installed
            .iter()
            .find(|p| p.identity == required_identity)
        {
            self.seen_identities.insert(required_identity.to_string());
            if !game_state::effective_enabled(self.ctx.states, &installed.identity) {
                self.steps
                    .push(PlanStep::Enable(installed.identity.clone()));
            }
            return;
        }
        match resolve::match_catalog(required_identity, self.ctx.catalog, self.ctx.records) {
            CatalogMatch::NoMatch => self.issues.push(Issue::MissingRequirement {
                plugin: requiring_plugin.to_string(),
                requires: required_identity.to_string(),
            }),
            CatalogMatch::Ambiguous(candidates) => self.issues.push(Issue::AmbiguousRequirement {
                plugin: requiring_plugin.to_string(),
                requires: required_identity.to_string(),
                candidates: candidates.iter().map(|e| e.name.clone()).collect(),
            }),
            CatalogMatch::Exact(found) => {
                let found = found.clone();
                self.stage_matched(found, false, required_identity);
            }
            CatalogMatch::Likely(found) => {
                let found = found.clone();
                self.stage_matched(found, true, required_identity);
            }
        }
    }

    /// A catalog match for an unmet requirement: dedup by catalog name (a diamond can reach
    /// the same entry via two different requirement strings before identities are known),
    /// then stage it and check the `Likely` promise against what was actually downloaded.
    fn stage_matched(&mut self, found: CatalogEntry, is_likely: bool, required_identity: &str) {
        if !self.seen_catalog_names.insert(found.name.clone()) {
            return;
        }
        let catalog_name = found.name.clone();
        match stage_download(self.ctx, &found) {
            Ok(staged) => {
                if is_likely && staged.identity != required_identity {
                    self.issues.push(Issue::IdentityMismatch {
                        expected: required_identity.to_string(),
                        catalog_name,
                        actual: staged.identity.clone(),
                    });
                }
                self.finish_staging(staged);
            }
            Err(error) => self.issues.push(Issue::CatalogDownloadFailed {
                catalog_name,
                error,
            }),
        }
    }

    fn stage_and_walk(&mut self, entry: &CatalogEntry) {
        match stage_download(self.ctx, entry) {
            Ok(staged) => self.finish_staging(staged),
            Err(error) => self.issues.push(Issue::CatalogDownloadFailed {
                catalog_name: entry.name.clone(),
                error,
            }),
        }
    }

    /// Recurses into the staged plugin's own `requires` (so dependencies land before it in
    /// `steps`), then records the staged plugin itself.
    fn finish_staging(&mut self, staged: StagedPlugin) {
        self.check_duplicate_identity(&staged);
        self.seen_identities.insert(staged.identity.clone());
        let identity = staged.identity.clone();
        let requires: Vec<String> = staged.meta.dependencies.requires.iter().cloned().collect();
        for requirement in &requires {
            self.resolve_requirement(requirement, &identity);
        }
        self.steps.push(PlanStep::Install(Box::new(staged)));
    }

    /// The game loads only the first folder with a given identity (decision A), so installing
    /// a staged plugin under a folder of its own would silently orphan an existing, differently
    /// named folder that already claims the same identity -- most often an unmanaged plugin
    /// decision B couldn't auto-adopt because its identity matched the catalog ambiguously.
    fn check_duplicate_identity(&mut self, staged: &StagedPlugin) {
        if let Some(existing) = self
            .ctx
            .installed
            .iter()
            .find(|p| p.identity == staged.identity && p.folder != staged.folder)
        {
            self.issues.push(Issue::DuplicateIdentity {
                identity: staged.identity.clone(),
                existing_folder: existing.folder.clone(),
                new_folder: staged.folder.clone(),
            });
        }
    }
}

/// Runs conflict and game-version checks, plus optional-dependency notes, over the state
/// `steps` would produce on top of what's currently installed/enabled.
fn finalize_plan(ctx: &PlanContext, steps: Vec<PlanStep>, mut issues: Vec<Issue>) -> InstallPlan {
    let mut forced: HashMap<&str, bool> = HashMap::new();
    for step in &steps {
        match step {
            PlanStep::Enable(identity) => {
                forced.insert(identity.as_str(), true);
            }
            PlanStep::Disable(identity) => {
                forced.insert(identity.as_str(), false);
            }
            PlanStep::Install(_) => {}
        }
    }
    let mut planned: Vec<PlannedPlugin> = ctx
        .installed
        .iter()
        .map(|p| {
            let enabled = forced
                .get(p.identity.as_str())
                .copied()
                .unwrap_or_else(|| game_state::effective_enabled(ctx.states, &p.identity));
            PlannedPlugin {
                identity: &p.identity,
                enabled,
                meta: &p.meta,
            }
        })
        .collect();
    for step in &steps {
        if let PlanStep::Install(staged) = step {
            let enabled = forced
                .get(staged.identity.as_str())
                .copied()
                .unwrap_or(true);
            planned.push(PlannedPlugin {
                identity: &staged.identity,
                enabled,
                meta: &staged.meta,
            });
        }
    }
    issues.extend(resolve::check_state(&planned, ctx.game_version));
    let notes = resolve::optional_notes(&planned);
    InstallPlan {
        steps,
        issues,
        notes,
    }
}

pub fn plan_install(entry: &CatalogEntry, ctx: &PlanContext) -> InstallPlan {
    tracing::info!(catalog_name = entry.name, "planning install");
    let mut builder = PlanBuilder::new(ctx);
    builder.stage_root(entry);
    let PlanBuilder { steps, issues, .. } = builder;
    let plan = finalize_plan(ctx, steps, issues);
    if plan.issues.is_empty() {
        tracing::info!(
            catalog_name = entry.name,
            steps = plan.steps.len(),
            "install plan ready"
        );
    } else {
        tracing::warn!(
            catalog_name = entry.name,
            issues = plan.issues.len(),
            "install plan has issues"
        );
    }
    plan
}

// ---------------------------------------------------------------------------
// Enable / disable / uninstall planning
// ---------------------------------------------------------------------------

fn planned_from_installed<'a>(
    ctx: &'a PlanContext,
    override_identity: Option<(&str, bool)>,
) -> Vec<PlannedPlugin<'a>> {
    ctx.installed
        .iter()
        .map(|p| {
            let enabled = match override_identity {
                Some((identity, forced)) if identity == p.identity => forced,
                _ => game_state::effective_enabled(ctx.states, &p.identity),
            };
            PlannedPlugin {
                identity: &p.identity,
                enabled,
                meta: &p.meta,
            }
        })
        .collect()
}

/// A catalog match offered for an unmet requirement, so the caller can propose installing it
/// instead of just reporting the block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequirementFix {
    Install(Box<CatalogEntry>),
    PickOne(Vec<CatalogEntry>),
}

#[derive(Debug)]
pub struct EnablePlan {
    pub identity: String,
    pub issues: Vec<Issue>,
    pub notes: Vec<Note>,
    pub requirement_fixes: HashMap<String, RequirementFix>,
    /// Added by [`EnablePlan::disable_to_resolve`] to resolve a conflict.
    pub to_disable: Vec<String>,
}

impl EnablePlan {
    /// Resolves a `Conflict` issue naming `identity` by disabling it as part of the same
    /// commit, instead of requiring an override. Mirrors `InstallPlan::disable_to_resolve`;
    /// no-op if `identity` isn't actually in conflict in this plan.
    pub fn disable_to_resolve(&mut self, identity: &str) {
        let in_conflict = |issue: &Issue| matches!(issue, Issue::Conflict { a, b } if a == identity || b == identity);
        if !self.issues.iter().any(in_conflict) {
            return;
        }
        self.issues.retain(|issue| !in_conflict(issue));
        self.to_disable.push(identity.to_string());
    }
}

pub fn plan_enable(identity: &str, ctx: &PlanContext) -> EnablePlan {
    tracing::info!(identity, "planning enable");
    let planned = planned_from_installed(ctx, Some((identity, true)));
    let mut issues = resolve::check_state(&planned, ctx.game_version);
    issues.extend(resolve::check_requirements(
        &planned,
        ctx.catalog,
        ctx.records,
    ));
    let notes = resolve::optional_notes(&planned);

    let mut requirement_fixes = HashMap::new();
    for issue in &issues {
        let requires = match issue {
            Issue::MissingRequirement { requires, .. } => requires,
            Issue::AmbiguousRequirement { requires, .. } => requires,
            _ => continue,
        };
        match resolve::match_catalog(requires, ctx.catalog, ctx.records) {
            CatalogMatch::Exact(e) | CatalogMatch::Likely(e) => {
                requirement_fixes.insert(
                    requires.clone(),
                    RequirementFix::Install(Box::new(e.clone())),
                );
            }
            CatalogMatch::Ambiguous(candidates) => {
                requirement_fixes.insert(
                    requires.clone(),
                    RequirementFix::PickOne(candidates.into_iter().cloned().collect()),
                );
            }
            CatalogMatch::NoMatch => {}
        }
    }
    if issues.is_empty() {
        tracing::info!(identity, "enable plan ready");
    } else {
        tracing::warn!(identity, issues = issues.len(), "enable plan has issues");
    }
    EnablePlan {
        identity: identity.to_string(),
        issues,
        notes,
        requirement_fixes,
        to_disable: Vec::new(),
    }
}

#[derive(Debug)]
pub struct DisablePlan {
    pub identity: String,
    pub issues: Vec<Issue>,
}

fn required_by_issue(identity: &str, planned: &[PlannedPlugin]) -> Vec<Issue> {
    let dependents = resolve::required_by(identity, planned);
    if dependents.is_empty() {
        Vec::new()
    } else {
        vec![Issue::RequiredBy {
            plugin: identity.to_string(),
            dependents,
        }]
    }
}

pub fn plan_disable(identity: &str, ctx: &PlanContext) -> DisablePlan {
    tracing::info!(identity, "planning disable");
    let planned = planned_from_installed(ctx, None);
    let issues = required_by_issue(identity, &planned);
    if !issues.is_empty() {
        tracing::warn!(identity, issues = issues.len(), "disable plan has issues");
    }
    DisablePlan {
        identity: identity.to_string(),
        issues,
    }
}

#[derive(Debug)]
pub struct UninstallPlan {
    pub folder: String,
    pub identity: String,
    pub issues: Vec<Issue>,
}

pub fn plan_uninstall(folder: &str, ctx: &PlanContext) -> UninstallPlan {
    tracing::info!(folder, "planning uninstall");
    let identity = ctx
        .installed
        .iter()
        .find(|p| p.folder == folder)
        .map_or_else(|| folder.to_string(), |p| p.identity.clone());
    let planned = planned_from_installed(ctx, None);
    let issues = required_by_issue(&identity, &planned);
    if !issues.is_empty() {
        tracing::warn!(folder, issues = issues.len(), "uninstall plan has issues");
    }
    UninstallPlan {
        folder: folder.to_string(),
        issues,
        identity,
    }
}

// ---------------------------------------------------------------------------
// Update planning and status
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct UpdatePlan {
    pub folder: String,
    staged: StagedPlugin,
    /// A requirement the new version adds that the old one didn't have, resolved the same
    /// recursive way `plan_install` resolves any requirement (diamonds staged once, cycles
    /// terminate, an installed-but-disabled match gets `Enable`d instead of reinstalled) --
    /// not just reported. Empty when the plugin being updated is currently disabled: nothing
    /// downstream needs its requirements satisfied either.
    new_requirements: Vec<PlanStep>,
    pub issues: Vec<Issue>,
    pub notes: Vec<Note>,
}

impl UpdatePlan {
    /// What the update installs or enables beyond the updated plugin itself, in dependency
    /// order, for a caller to show in a review before anything is committed.
    pub fn new_requirements(&self) -> &[PlanStep] {
        &self.new_requirements
    }
}

/// Fetches the catalog's current version of an already-managed plugin, recursively resolves
/// any requirement the new version adds that the old one didn't have (the same walk
/// `plan_install` does for a fresh install), and re-checks conflicts/game-version/remaining
/// requirements over the resulting state.
pub fn plan_update(folder: &str, ctx: &PlanContext) -> Result<UpdatePlan, String> {
    tracing::info!(folder, "planning update");
    let current = ctx
        .installed
        .iter()
        .find(|p| p.folder == folder)
        .ok_or_else(|| format!("{folder:?} is not installed"))?;
    let record = current
        .record
        .as_ref()
        .ok_or_else(|| format!("{folder:?} has no install record (not managed by this tool)"))?;
    let entry = ctx
        .catalog
        .iter()
        .find(|e| e.name == record.catalog_name)
        .ok_or_else(|| format!("{:?} is no longer in the catalog", record.catalog_name))?;
    let staged = stage_download(ctx, entry).inspect_err(|e| {
        tracing::warn!(folder, error = e, "update plan failed to download");
    })?;

    let currently_enabled = game_state::effective_enabled(ctx.states, &current.identity);
    // Only walk the new version's requirements if it's actually going to be enabled; nothing
    // downstream needs them satisfied otherwise, matching the existing
    // `check_requirements` guard below.
    let mut builder = PlanBuilder::new(ctx);
    if currently_enabled {
        builder.seen_identities.insert(staged.identity.clone());
        let requires: Vec<String> = staged.meta.dependencies.requires.iter().cloned().collect();
        for requirement in &requires {
            builder.resolve_requirement(requirement, &staged.identity);
        }
    }
    let PlanBuilder {
        steps: new_requirements,
        issues: mut requirement_issues,
        ..
    } = builder;

    let mut forced: HashMap<&str, bool> = HashMap::new();
    for step in &new_requirements {
        match step {
            PlanStep::Enable(identity) => {
                forced.insert(identity.as_str(), true);
            }
            PlanStep::Disable(identity) => {
                forced.insert(identity.as_str(), false);
            }
            PlanStep::Install(_) => {}
        }
    }
    let mut planned: Vec<PlannedPlugin> = ctx
        .installed
        .iter()
        .filter(|p| p.folder != folder)
        .map(|p| {
            let enabled = forced
                .get(p.identity.as_str())
                .copied()
                .unwrap_or_else(|| game_state::effective_enabled(ctx.states, &p.identity));
            PlannedPlugin {
                identity: &p.identity,
                enabled,
                meta: &p.meta,
            }
        })
        .collect();
    planned.push(PlannedPlugin {
        identity: &staged.identity,
        enabled: currently_enabled,
        meta: &staged.meta,
    });
    for step in &new_requirements {
        if let PlanStep::Install(dep) = step {
            let enabled = forced.get(dep.identity.as_str()).copied().unwrap_or(true);
            planned.push(PlannedPlugin {
                identity: &dep.identity,
                enabled,
                meta: &dep.meta,
            });
        }
    }

    let mut issues = resolve::check_state(&planned, ctx.game_version);
    issues.append(&mut requirement_issues);
    if currently_enabled {
        // A safety net over the whole final state (e.g. another plugin's requirement the
        // update stops satisfying); anything the walk above already reported isn't repeated.
        for issue in resolve::check_requirements(&planned, ctx.catalog, ctx.records) {
            if !issues.contains(&issue) {
                issues.push(issue);
            }
        }
    }
    let notes = resolve::optional_notes(&planned);
    if issues.is_empty() {
        tracing::info!(folder, "update plan ready");
    } else {
        tracing::warn!(folder, issues = issues.len(), "update plan has issues");
    }
    Ok(UpdatePlan {
        folder: folder.to_string(),
        staged,
        new_requirements,
        issues,
        notes,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    UpToDate,
    Available {
        from: String,
        to: String,
    },
    /// The record's version is empty (decision B: an adopted plugin's version is unknown).
    Unknown,
    NotInCatalog,
}

/// String equality only (decision D): catalog versions aren't orderable (tags, commit SHAs).
pub fn update_status(record: &InstallRecord, catalog: &[CatalogEntry]) -> UpdateStatus {
    if record.version.is_empty() {
        return UpdateStatus::Unknown;
    }
    match catalog.iter().find(|e| e.name == record.catalog_name) {
        None => UpdateStatus::NotInCatalog,
        Some(entry) if entry.version == record.version => UpdateStatus::UpToDate,
        Some(entry) => UpdateStatus::Available {
            from: record.version.clone(),
            to: entry.version.clone(),
        },
    }
}

// ---------------------------------------------------------------------------
// Unmanaged adoption (decision B)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmanagedPlugin {
    pub folder: String,
    pub identity: String,
    /// Empty for no catalog hits at all; more than one for an ambiguous normalized match.
    pub candidates: Vec<CatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdoptionResult {
    /// Ready to merge into `records.json` as-is.
    pub adopted: Vec<InstallRecord>,
    pub needs_user: Vec<UnmanagedPlugin>,
}

/// `Exact`/`Likely` catalog matches are adopted automatically; the adopted record's version
/// is left empty since we can't know which version the user actually dropped in, which makes
/// `update_status` report `Unknown` until a reinstall pins it down. Never deletes or moves an
/// unmanaged plugin.
pub fn adopt_candidates(
    installed: &[InstalledPlugin],
    catalog: &[CatalogEntry],
    records: &InstallRecords,
) -> AdoptionResult {
    let mut result = AdoptionResult::default();
    for plugin in installed.iter().filter(|p| p.record.is_none()) {
        match resolve::match_catalog(&plugin.identity, catalog, records) {
            CatalogMatch::Exact(entry) | CatalogMatch::Likely(entry) => {
                result.adopted.push(InstallRecord {
                    catalog_name: entry.name.clone(),
                    folder: plugin.folder.clone(),
                    identity: plugin.identity.clone(),
                    version: String::new(),
                    source_url: entry.url.clone(),
                    sha256: String::new(),
                });
            }
            CatalogMatch::Ambiguous(candidates) => result.needs_user.push(UnmanagedPlugin {
                folder: plugin.folder.clone(),
                identity: plugin.identity.clone(),
                candidates: candidates.into_iter().cloned().collect(),
            }),
            CatalogMatch::NoMatch => result.needs_user.push(UnmanagedPlugin {
                folder: plugin.folder.clone(),
                identity: plugin.identity.clone(),
                candidates: Vec::new(),
            }),
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Commit
// ---------------------------------------------------------------------------

/// Paths to the manager's own on-disk state. Loaded fresh and saved by `commit*`, so commit
/// always acts on what's actually on disk rather than a possibly-stale planning-time snapshot.
pub struct CommitContext<'a> {
    pub config_dir: &'a Path,
    pub records_path: &'a Path,
    pub profiles_path: &'a Path,
    /// A seam so tests can simulate the game running without a real process; production code
    /// always uses [`new`](Self::new), which wires in `game_state::detect_game_process`.
    pub detect_game: fn() -> GameProcess,
}

impl<'a> CommitContext<'a> {
    pub fn new(config_dir: &'a Path, records_path: &'a Path, profiles_path: &'a Path) -> Self {
        Self {
            config_dir,
            records_path,
            profiles_path,
            detect_game: game_state::detect_game_process,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommitReport {
    pub installed: Vec<Installed>,
    pub enabled: Vec<String>,
    pub disabled: Vec<String>,
    pub backup: Option<PathBuf>,
    /// The game process couldn't be detected; plugins.txt was written anyway (decision F),
    /// but the UI should warn it may be overwritten if the game is in fact running.
    pub game_detection_failed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CommitError {
    #[error("{} blocking issue(s) remain; override to proceed anyway", .0.len())]
    Blocked(Vec<Issue>),
    #[error("Endless Sky is running; close it first")]
    GameRunning,
    /// An install step failed partway through a multi-step plan (e.g. a `requires` chain).
    /// `report` holds exactly what succeeded before it, already written to disk.
    #[error("install of {folder:?} failed: {error}")]
    InstallFailed {
        report: Box<CommitReport>,
        folder: String,
        error: InstallError,
    },
    #[error("uninstall failed: {0}")]
    Uninstall(InstallError),
    #[error("{0}")]
    Io(String),
}

#[derive(Default)]
struct ChangeSet<'a> {
    upsert_records: &'a [InstallRecord],
    remove_records: &'a [String],
    to_enable: &'a [String],
    to_disable: &'a [String],
    remove_states: &'a [String],
}

/// Merges `changes` into records.json, plugins.txt and the active profile, in that order,
/// and saves all three. The one place that touches the manager's on-disk state, so every
/// `commit*` function (including the partial-failure path) goes through it.
fn persist_changes(
    ctx: &CommitContext,
    changes: ChangeSet,
    game: GameProcess,
) -> Result<game_state::WriteOutcome, CommitError> {
    let mut records =
        records::load(ctx.records_path).map_err(|e| CommitError::Io(e.to_string()))?;
    for record in changes.upsert_records {
        records.insert(record.folder.clone(), record.clone());
    }
    for folder in changes.remove_records {
        records.remove(folder);
    }
    records::save(ctx.records_path, &records).map_err(|e| CommitError::Io(e.to_string()))?;

    let mut states = game_state::read_plugin_states(ctx.config_dir)
        .map_err(|e| CommitError::Io(e.to_string()))?;
    for identity in changes.to_enable {
        states.insert(identity.clone(), true);
    }
    for identity in changes.to_disable {
        states.insert(identity.clone(), false);
    }
    for identity in changes.remove_states {
        states.remove(identity);
    }
    let outcome = match game_state::write_plugin_states(ctx.config_dir, &states, game) {
        Ok(outcome) => outcome,
        Err(game_state::WriteError::GameRunning) => return Err(CommitError::GameRunning),
        Err(game_state::WriteError::Io(e)) => return Err(CommitError::Io(e.to_string())),
    };

    let mut profiles =
        ProfileStore::load(ctx.profiles_path).map_err(|e| CommitError::Io(e.to_string()))?;
    for record in changes.upsert_records {
        profiles.on_installed(&record.identity, Some(&record.catalog_name));
    }
    if let Some(profile) = profiles.active_profile_mut() {
        for identity in changes.to_disable.iter().chain(changes.remove_states) {
            profile.enabled.remove(identity);
        }
    }
    profiles
        .save(ctx.profiles_path)
        .map_err(|e| CommitError::Io(e.to_string()))?;

    Ok(outcome)
}

fn refuse_if_blocked(issues: &[Issue], override_issues: bool) -> Result<(), CommitError> {
    if !override_issues && !issues.is_empty() {
        return Err(CommitError::Blocked(issues.to_vec()));
    }
    Ok(())
}

/// The game must not be running before anything is touched (decision F: it would overwrite
/// `plugins.txt` on exit, and we must never half-apply a plan).
fn refuse_if_game_running(detect_game: fn() -> GameProcess) -> Result<GameProcess, CommitError> {
    let game = detect_game();
    if game == GameProcess::Running {
        return Err(CommitError::GameRunning);
    }
    Ok(game)
}

fn report_from(
    outcome: game_state::WriteOutcome,
    installed: Vec<Installed>,
    enabled: Vec<String>,
    disabled: Vec<String>,
) -> CommitReport {
    CommitReport {
        installed,
        enabled,
        disabled,
        backup: outcome.backup,
        game_detection_failed: outcome.detection_failed,
    }
}

/// Logs the outcome of any `commit*` function uniformly; `op` names which one.
fn log_commit(op: &'static str, result: &Result<CommitReport, CommitError>) {
    match result {
        Ok(r) => tracing::info!(
            op,
            installed = r.installed.len(),
            enabled = r.enabled.len(),
            disabled = r.disabled.len(),
            "commit succeeded"
        ),
        Err(CommitError::Blocked(issues)) => {
            tracing::warn!(
                op,
                issues = issues.len(),
                "commit blocked by unresolved issues"
            )
        }
        Err(CommitError::GameRunning) => tracing::warn!(op, "commit refused: game is running"),
        Err(e) => tracing::error!(op, error = %e, "commit failed"),
    }
}

/// Installs every staged plugin in order (dependencies first), then writes records,
/// `plugins.txt` and the active profile. If a step fails partway, everything before it is
/// still persisted (so the manager's own state matches what's actually on disk) and the
/// failing folder/error are reported; this isn't a multi-plugin transaction, just an honest
/// account of what happened.
pub fn commit(
    plan: InstallPlan,
    ctx: &CommitContext,
    override_issues: bool,
) -> Result<CommitReport, CommitError> {
    tracing::info!("committing install");
    let result = commit_inner(plan, ctx, override_issues);
    log_commit("install", &result);
    result
}

fn commit_inner(
    plan: InstallPlan,
    ctx: &CommitContext,
    override_issues: bool,
) -> Result<CommitReport, CommitError> {
    refuse_if_blocked(&plan.issues, override_issues)?;
    let game = refuse_if_game_running(ctx.detect_game)?;

    let plugins_dir = install::plugins_dir(ctx.config_dir);
    let tmp_dir = install::tmp_dir(ctx.config_dir);

    let mut installed = Vec::new();
    let mut new_records = Vec::new();
    let mut to_enable = Vec::new();
    let mut to_disable = Vec::new();

    for step in plan.steps {
        match step {
            PlanStep::Install(staged) => {
                match install::install(&staged.root, &plugins_dir, &tmp_dir, &staged.folder) {
                    Ok(result) => {
                        to_enable.push(result.identity().to_string());
                        new_records.push(InstallRecord {
                            catalog_name: staged.catalog_name,
                            folder: staged.folder,
                            identity: result.identity().to_string(),
                            version: staged.version,
                            source_url: staged.source_url,
                            sha256: staged.sha256,
                        });
                        installed.push(result);
                    }
                    Err(error) => {
                        let folder = staged.folder;
                        let outcome = persist_changes(
                            ctx,
                            ChangeSet {
                                upsert_records: &new_records,
                                to_enable: &to_enable,
                                to_disable: &to_disable,
                                ..Default::default()
                            },
                            game,
                        )?;
                        return Err(CommitError::InstallFailed {
                            report: Box::new(report_from(
                                outcome, installed, to_enable, to_disable,
                            )),
                            folder,
                            error,
                        });
                    }
                }
            }
            PlanStep::Enable(identity) => to_enable.push(identity),
            PlanStep::Disable(identity) => to_disable.push(identity),
        }
    }
    let outcome = persist_changes(
        ctx,
        ChangeSet {
            upsert_records: &new_records,
            to_enable: &to_enable,
            to_disable: &to_disable,
            ..Default::default()
        },
        game,
    )?;
    Ok(report_from(outcome, installed, to_enable, to_disable))
}

pub fn commit_enable(
    plan: EnablePlan,
    ctx: &CommitContext,
    override_issues: bool,
) -> Result<CommitReport, CommitError> {
    tracing::info!(identity = plan.identity, "committing enable");
    let result = commit_enable_inner(plan, ctx, override_issues);
    log_commit("enable", &result);
    result
}

fn commit_enable_inner(
    plan: EnablePlan,
    ctx: &CommitContext,
    override_issues: bool,
) -> Result<CommitReport, CommitError> {
    refuse_if_blocked(&plan.issues, override_issues)?;
    let game = refuse_if_game_running(ctx.detect_game)?;
    let to_enable = [plan.identity];
    let outcome = persist_changes(
        ctx,
        ChangeSet {
            to_enable: &to_enable,
            to_disable: &plan.to_disable,
            ..Default::default()
        },
        game,
    )?;
    Ok(report_from(
        outcome,
        Vec::new(),
        to_enable.to_vec(),
        plan.to_disable,
    ))
}

pub fn commit_disable(
    plan: DisablePlan,
    ctx: &CommitContext,
    override_issues: bool,
) -> Result<CommitReport, CommitError> {
    tracing::info!(identity = plan.identity, "committing disable");
    let result = commit_disable_inner(plan, ctx, override_issues);
    log_commit("disable", &result);
    result
}

fn commit_disable_inner(
    plan: DisablePlan,
    ctx: &CommitContext,
    override_issues: bool,
) -> Result<CommitReport, CommitError> {
    refuse_if_blocked(&plan.issues, override_issues)?;
    let game = refuse_if_game_running(ctx.detect_game)?;
    let to_disable = [plan.identity];
    let outcome = persist_changes(
        ctx,
        ChangeSet {
            to_disable: &to_disable,
            ..Default::default()
        },
        game,
    )?;
    Ok(report_from(
        outcome,
        Vec::new(),
        Vec::new(),
        to_disable.to_vec(),
    ))
}

pub fn commit_uninstall(
    plan: UninstallPlan,
    ctx: &CommitContext,
    override_issues: bool,
) -> Result<CommitReport, CommitError> {
    tracing::info!(folder = plan.folder, "committing uninstall");
    let result = commit_uninstall_inner(plan, ctx, override_issues);
    log_commit("uninstall", &result);
    result
}

fn commit_uninstall_inner(
    plan: UninstallPlan,
    ctx: &CommitContext,
    override_issues: bool,
) -> Result<CommitReport, CommitError> {
    refuse_if_blocked(&plan.issues, override_issues)?;
    let game = refuse_if_game_running(ctx.detect_game)?;
    let plugins_dir = install::plugins_dir(ctx.config_dir);
    let tmp_dir = install::tmp_dir(ctx.config_dir);
    install::uninstall(&plugins_dir, &tmp_dir, &plan.folder).map_err(CommitError::Uninstall)?;

    let remove_records = [plan.folder];
    let remove_states = [plan.identity];
    let outcome = persist_changes(
        ctx,
        ChangeSet {
            remove_records: &remove_records,
            remove_states: &remove_states,
            ..Default::default()
        },
        game,
    )?;
    Ok(report_from(
        outcome,
        Vec::new(),
        Vec::new(),
        remove_states.to_vec(),
    ))
}

/// Replaces the folder in place and keeps the updated plugin's own enabled state: it's never
/// added to `to_enable`/`to_disable`, so whatever `plugins.txt` already said for its identity
/// stands. A brand-new requirement the new version adds (`plan.new_requirements`) is a
/// different matter -- freshly installed like any other install, enabled like one, or
/// enabled-in-place if it turns out to already be installed but disabled -- installed first,
/// so a failure partway through still persists whatever succeeded, including the update
/// itself if its own install step came after the failure.
pub fn commit_update(
    plan: UpdatePlan,
    ctx: &CommitContext,
    override_issues: bool,
) -> Result<CommitReport, CommitError> {
    tracing::info!(folder = plan.folder, "committing update");
    let result = commit_update_inner(plan, ctx, override_issues);
    log_commit("update", &result);
    result
}

fn commit_update_inner(
    plan: UpdatePlan,
    ctx: &CommitContext,
    override_issues: bool,
) -> Result<CommitReport, CommitError> {
    refuse_if_blocked(&plan.issues, override_issues)?;
    let game = refuse_if_game_running(ctx.detect_game)?;
    let plugins_dir = install::plugins_dir(ctx.config_dir);
    let tmp_dir = install::tmp_dir(ctx.config_dir);

    let mut installed = Vec::new();
    let mut upsert_records = Vec::new();
    let mut to_enable = Vec::new();
    let mut to_disable = Vec::new();

    for step in plan.new_requirements {
        match step {
            PlanStep::Install(staged) => {
                match install::install(&staged.root, &plugins_dir, &tmp_dir, &staged.folder) {
                    Ok(result) => {
                        to_enable.push(result.identity().to_string());
                        upsert_records.push(InstallRecord {
                            catalog_name: staged.catalog_name,
                            folder: staged.folder,
                            identity: result.identity().to_string(),
                            version: staged.version,
                            source_url: staged.source_url,
                            sha256: staged.sha256,
                        });
                        installed.push(result);
                    }
                    Err(error) => {
                        let folder = staged.folder;
                        let outcome = persist_changes(
                            ctx,
                            ChangeSet {
                                upsert_records: &upsert_records,
                                to_enable: &to_enable,
                                to_disable: &to_disable,
                                ..Default::default()
                            },
                            game,
                        )?;
                        return Err(CommitError::InstallFailed {
                            report: Box::new(report_from(
                                outcome, installed, to_enable, to_disable,
                            )),
                            folder,
                            error,
                        });
                    }
                }
            }
            PlanStep::Enable(identity) => to_enable.push(identity),
            PlanStep::Disable(identity) => to_disable.push(identity),
        }
    }

    let staged = plan.staged;
    let folder = staged.folder.clone();
    match install::install(&staged.root, &plugins_dir, &tmp_dir, &staged.folder) {
        Ok(result) => {
            upsert_records.push(InstallRecord {
                catalog_name: staged.catalog_name,
                folder,
                identity: result.identity().to_string(),
                version: staged.version,
                source_url: staged.source_url,
                sha256: staged.sha256,
            });
            installed.push(result);
        }
        Err(error) => {
            let outcome = persist_changes(
                ctx,
                ChangeSet {
                    upsert_records: &upsert_records,
                    to_enable: &to_enable,
                    to_disable: &to_disable,
                    ..Default::default()
                },
                game,
            )?;
            return Err(CommitError::InstallFailed {
                report: Box::new(report_from(outcome, installed, to_enable, to_disable)),
                folder,
                error,
            });
        }
    }

    let outcome = persist_changes(
        ctx,
        ChangeSet {
            upsert_records: &upsert_records,
            to_enable: &to_enable,
            to_disable: &to_disable,
            ..Default::default()
        },
        game,
    )?;
    Ok(report_from(outcome, installed, to_enable, to_disable))
}

/// Commits every staged update in order, the same partial-failure honesty as [`commit`]: if
/// one plugin's install fails partway through the batch, every update before it is still
/// persisted and the failing folder/error are reported, rather than losing successful work
/// to an unrelated later failure.
///
/// Unlike [`commit`], an updated plugin's own enabled state is never touched (same as
/// [`commit_update`]): whatever `plugins.txt` already says for each identity stands. A new
/// requirement one of them adds is installed and enabled like any fresh install, though --
/// see [`commit_update`]'s doc comment.
pub fn commit_update_all(
    plans: Vec<UpdatePlan>,
    ctx: &CommitContext,
    override_issues: bool,
) -> Result<CommitReport, CommitError> {
    tracing::info!(count = plans.len(), "committing update-all");
    let result = commit_update_all_inner(plans, ctx, override_issues);
    log_commit("update-all", &result);
    result
}

fn commit_update_all_inner(
    plans: Vec<UpdatePlan>,
    ctx: &CommitContext,
    override_issues: bool,
) -> Result<CommitReport, CommitError> {
    let all_issues: Vec<Issue> = plans
        .iter()
        .flat_map(|p| p.issues.iter().cloned())
        .collect();
    refuse_if_blocked(&all_issues, override_issues)?;
    let game = refuse_if_game_running(ctx.detect_game)?;

    let plugins_dir = install::plugins_dir(ctx.config_dir);
    let tmp_dir = install::tmp_dir(ctx.config_dir);

    let mut installed = Vec::new();
    let mut upsert_records = Vec::new();
    let mut to_enable = Vec::new();
    let mut to_disable = Vec::new();

    for plan in plans {
        for step in plan.new_requirements {
            match step {
                PlanStep::Install(staged) => {
                    match install::install(&staged.root, &plugins_dir, &tmp_dir, &staged.folder) {
                        Ok(result) => {
                            to_enable.push(result.identity().to_string());
                            upsert_records.push(InstallRecord {
                                catalog_name: staged.catalog_name,
                                folder: staged.folder,
                                identity: result.identity().to_string(),
                                version: staged.version,
                                source_url: staged.source_url,
                                sha256: staged.sha256,
                            });
                            installed.push(result);
                        }
                        Err(error) => {
                            let folder = staged.folder;
                            let outcome = persist_changes(
                                ctx,
                                ChangeSet {
                                    upsert_records: &upsert_records,
                                    to_enable: &to_enable,
                                    to_disable: &to_disable,
                                    ..Default::default()
                                },
                                game,
                            )?;
                            return Err(CommitError::InstallFailed {
                                report: Box::new(report_from(
                                    outcome, installed, to_enable, to_disable,
                                )),
                                folder,
                                error,
                            });
                        }
                    }
                }
                PlanStep::Enable(identity) => to_enable.push(identity),
                PlanStep::Disable(identity) => to_disable.push(identity),
            }
        }

        let staged = plan.staged;
        let folder = staged.folder.clone();
        match install::install(&staged.root, &plugins_dir, &tmp_dir, &staged.folder) {
            Ok(result) => {
                upsert_records.push(InstallRecord {
                    catalog_name: staged.catalog_name,
                    folder,
                    identity: result.identity().to_string(),
                    version: staged.version,
                    source_url: staged.source_url,
                    sha256: staged.sha256,
                });
                installed.push(result);
            }
            Err(error) => {
                let outcome = persist_changes(
                    ctx,
                    ChangeSet {
                        upsert_records: &upsert_records,
                        to_enable: &to_enable,
                        to_disable: &to_disable,
                        ..Default::default()
                    },
                    game,
                )?;
                return Err(CommitError::InstallFailed {
                    report: Box::new(report_from(outcome, installed, to_enable, to_disable)),
                    folder,
                    error,
                });
            }
        }
    }
    let outcome = persist_changes(
        ctx,
        ChangeSet {
            upsert_records: &upsert_records,
            to_enable: &to_enable,
            to_disable: &to_disable,
            ..Default::default()
        },
        game,
    )?;
    Ok(report_from(outcome, installed, to_enable, to_disable))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_skips_non_plugin_entries_and_zips() {
        let dir = tempfile::tempdir().unwrap();
        let plugins = dir.path().join("plugins");
        fs::create_dir_all(plugins.join("Real/data")).unwrap();
        fs::write(plugins.join("Real/plugin.txt"), "name \"Real Plugin\"\n").unwrap();
        fs::create_dir_all(plugins.join("NotAPlugin")).unwrap();
        fs::write(plugins.join("loose.zip"), "not actually a zip").unwrap();

        let found = scan_installed(&plugins, &InstallRecords::new()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].folder, "Real");
        assert_eq!(found[0].identity, "Real Plugin");
        assert!(found[0].record.is_none());
    }

    #[test]
    fn missing_plugins_dir_scans_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            scan_installed(&dir.path().join("plugins"), &InstallRecords::new())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn update_status_cases() {
        fn entry(name: &str, version: &str) -> CatalogEntry {
            CatalogEntry {
                name: name.into(),
                authors: "A".into(),
                homepage: "https://example.com".into(),
                license: "MIT".into(),
                version: version.into(),
                short_description: "".into(),
                description: None,
                url: "https://example.com/a.zip".into(),
                icon_url: None,
                autoupdate: None,
            }
        }
        fn record(catalog_name: &str, version: &str) -> InstallRecord {
            InstallRecord {
                catalog_name: catalog_name.into(),
                folder: catalog_name.into(),
                identity: catalog_name.into(),
                version: version.into(),
                source_url: "https://example.com/a.zip".into(),
                sha256: String::new(),
            }
        }
        let catalog = [entry("Foo", "v2")];
        assert_eq!(
            update_status(&record("Foo", "v2"), &catalog),
            UpdateStatus::UpToDate
        );
        assert_eq!(
            update_status(&record("Foo", "v1"), &catalog),
            UpdateStatus::Available {
                from: "v1".into(),
                to: "v2".into()
            }
        );
        assert_eq!(
            update_status(&record("Foo", ""), &catalog),
            UpdateStatus::Unknown
        );
        assert_eq!(
            update_status(&record("Gone", "v1"), &catalog),
            UpdateStatus::NotInCatalog
        );
    }
}
