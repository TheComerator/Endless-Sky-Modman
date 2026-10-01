//! Everything the frontend receives, as plain serializable data.
//!
//! `esmm-core`'s own types are deliberately not `Serialize` (they're library types, some of
//! them own temp dirs), so each one the UI needs gets a view here. Every type derives
//! `ts_rs::TS` and is exported to `app/src/bindings/` whenever `cargo test` runs (the export
//! path is set in the repo's `.cargo/config.toml`), so the TypeScript side can never drift
//! from what Rust actually sends. Field names are camelCase on the wire; tagged enums use a
//! `kind` field.

use std::time::{SystemTime, UNIX_EPOCH};

use esmm_core::catalog::{CatalogEntry, CatalogFetch, FetchSource};
use esmm_core::game_install::{GameInstall, InstallKind, Launch};
use esmm_core::game_state::GameProcess;
use esmm_core::manager::{CommitReport, InstalledPlugin, UpdateStatus};
use esmm_core::profiles::{Drift, Missing};
use esmm_core::resolve::{Issue, Note};
use serde::Serialize;
use ts_rs::TS;

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CatalogEntryView {
    pub name: String,
    pub authors: String,
    pub homepage: String,
    pub license: String,
    pub version: String,
    pub short_description: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
}

impl From<&CatalogEntry> for CatalogEntryView {
    fn from(e: &CatalogEntry) -> Self {
        CatalogEntryView {
            name: e.name.clone(),
            authors: e.authors.clone(),
            homepage: e.homepage.clone(),
            license: e.license.clone(),
            version: e.version.clone(),
            short_description: e.short_description.clone(),
            description: e.description.clone(),
            icon_url: e.icon_url.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum CatalogSourceView {
    Fresh,
    NotModified,
    /// The network request failed; this is the cached copy.
    Offline {
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CatalogView {
    pub entries: Vec<CatalogEntryView>,
    pub source: CatalogSourceView,
    /// Unix seconds of the fetch that produced `entries` (the cache's time when offline).
    #[ts(type = "number")]
    pub fetched_at: u64,
}

impl From<&CatalogFetch> for CatalogView {
    fn from(fetch: &CatalogFetch) -> Self {
        CatalogView {
            entries: fetch.entries.iter().map(CatalogEntryView::from).collect(),
            source: match &fetch.source {
                FetchSource::Fresh => CatalogSourceView::Fresh,
                FetchSource::NotModified => CatalogSourceView::NotModified,
                FetchSource::Offline { error } => CatalogSourceView::Offline {
                    error: error.clone(),
                },
            },
            fetched_at: unix_secs(fetch.fetched_at),
        }
    }
}

fn unix_secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

// ---------------------------------------------------------------------------
// Game installs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum InstallKindView {
    Standalone,
    Steam,
    Flatpak,
    Custom,
}

impl From<InstallKind> for InstallKindView {
    fn from(kind: InstallKind) -> Self {
        match kind {
            InstallKind::Standalone => InstallKindView::Standalone,
            InstallKind::Steam => InstallKindView::Steam,
            InstallKind::Flatpak => InstallKindView::Flatpak,
            InstallKind::Custom => InstallKindView::Custom,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstallView {
    /// Stable key for selecting this install (see `settings::install_key`).
    pub key: String,
    pub kind: InstallKindView,
    pub config_dir: String,
    pub executable: Option<String>,
    /// How the launch button would start it; `None` means there's no known way.
    pub launch: Option<String>,
    /// Added by the user rather than auto-detected (decision H's "let the user override").
    pub user_added: bool,
    /// `endless-sky --version`, when it could be read.
    pub game_version: Option<String>,
}

impl InstallView {
    pub fn new(
        key: String,
        install: &GameInstall,
        user_added: bool,
        game_version: Option<String>,
    ) -> Self {
        InstallView {
            key,
            kind: install.kind.into(),
            config_dir: install.config_dir.display().to_string(),
            executable: install.executable.as_ref().map(|p| p.display().to_string()),
            launch: match &install.launch {
                Launch::Executable { program, .. } => Some(program.display().to_string()),
                Launch::Steam => Some("Steam".into()),
                Launch::Flatpak => Some("Flatpak".into()),
                Launch::Unknown => None,
            },
            user_added,
            game_version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstallsView {
    pub installs: Vec<InstallView>,
    /// The install every other command acts on; `None` when nothing was found or added.
    pub selected: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum GameProcessView {
    Running,
    NotRunning,
    /// The process list couldn't be read: writes still go ahead (decision F), with a warning.
    Unknown,
}

impl From<GameProcess> for GameProcessView {
    fn from(p: GameProcess) -> Self {
        match p {
            GameProcess::Running => GameProcessView::Running,
            GameProcess::NotRunning => GameProcessView::NotRunning,
            GameProcess::Unknown => GameProcessView::Unknown,
        }
    }
}

// ---------------------------------------------------------------------------
// Installed plugins and profiles
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum UpdateView {
    UpToDate,
    Available {
        from: String,
        to: String,
    },
    /// Adopted plugin: we can't know which version was dropped in (decision B).
    Unknown,
    NotInCatalog,
    /// Not installed by the manager and not linked to a catalog entry.
    Unmanaged,
    /// The catalog isn't loaded yet, so there's nothing to compare against.
    Unchecked,
}

impl From<UpdateStatus> for UpdateView {
    fn from(status: UpdateStatus) -> Self {
        match status {
            UpdateStatus::UpToDate => UpdateView::UpToDate,
            UpdateStatus::Available { from, to } => UpdateView::Available { from, to },
            UpdateStatus::Unknown => UpdateView::Unknown,
            UpdateStatus::NotInCatalog => UpdateView::NotInCatalog,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstalledPluginView {
    pub folder: String,
    pub identity: String,
    pub enabled: bool,
    /// Has an install record (installed or adopted by the manager).
    pub managed: bool,
    pub catalog_name: Option<String>,
    /// The record's version; empty for an adopted plugin.
    pub installed_version: Option<String>,
    /// `plugin.txt`'s own `version`, purely informational.
    pub plugin_version: Option<String>,
    pub update: UpdateView,
    pub about: String,
    pub authors: Vec<String>,
    pub requires: Vec<String>,
    pub optional: Vec<String>,
    pub conflicts: Vec<String>,
    pub game_version: Option<String>,
}

impl InstalledPluginView {
    pub fn new(plugin: &InstalledPlugin, enabled: bool, update: UpdateView) -> Self {
        let deps = &plugin.meta.dependencies;
        InstalledPluginView {
            folder: plugin.folder.clone(),
            identity: plugin.identity.clone(),
            enabled,
            managed: plugin.record.is_some(),
            catalog_name: plugin.record.as_ref().map(|r| r.catalog_name.clone()),
            installed_version: plugin.record.as_ref().map(|r| r.version.clone()),
            plugin_version: plugin.meta.version.clone(),
            update,
            about: plugin.meta.about.trim_end().to_string(),
            authors: plugin.meta.authors.iter().cloned().collect(),
            requires: deps.requires.iter().cloned().collect(),
            optional: deps.optional.iter().cloned().collect(),
            conflicts: deps.conflicts.iter().cloned().collect(),
            game_version: deps.game_version.clone(),
        }
    }
}

/// An unmanaged plugin with no single catalog match (decision B: "ask the user").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UnmanagedView {
    pub folder: String,
    pub identity: String,
    /// Empty: not in the catalog at all. Several: ambiguous, the user picks one.
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MissingView {
    pub identity: String,
    pub catalog_name: Option<String>,
}

impl From<&Missing> for MissingView {
    fn from(m: &Missing) -> Self {
        MissingView {
            identity: m.identity.clone(),
            catalog_name: m.catalog_name.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DriftView {
    pub enabled_but_not_in_profile: Vec<String>,
    pub in_profile_but_disabled: Vec<String>,
}

impl From<&Drift> for DriftView {
    fn from(d: &Drift) -> Self {
        DriftView {
            enabled_but_not_in_profile: d.enabled_but_not_in_profile.iter().cloned().collect(),
            in_profile_but_disabled: d.in_profile_but_disabled.iter().cloned().collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProfileSummary {
    pub name: String,
    pub enabled_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProfilesView {
    pub active: Option<String>,
    pub profiles: Vec<ProfileSummary>,
    /// Live `plugins.txt` vs the active profile (e.g. toggled in-game).
    pub drift: DriftView,
    /// Plugins the active profile enables that aren't installed.
    pub missing: Vec<MissingView>,
}

/// One call's worth of everything the main views render, read fresh from disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ManagerState {
    pub install: Option<InstallView>,
    pub game: GameProcessView,
    pub plugins: Vec<InstalledPluginView>,
    pub unmanaged: Vec<UnmanagedView>,
    pub profiles: ProfilesView,
    /// Folders adopted automatically during this call (decision B), for a notice.
    pub adopted: Vec<String>,
    /// The catalog wasn't loaded yet, so update status and adoption were skipped.
    pub catalog_loaded: bool,
}

// ---------------------------------------------------------------------------
// Plans
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum IssueView {
    MissingRequirement {
        plugin: String,
        requires: String,
    },
    AmbiguousRequirement {
        plugin: String,
        requires: String,
        candidates: Vec<String>,
    },
    IdentityMismatch {
        expected: String,
        catalog_name: String,
        actual: String,
    },
    Conflict {
        a: String,
        b: String,
    },
    GameTooOld {
        plugin: String,
        required: String,
        actual: String,
    },
    GameVersionUnknown {
        plugin: String,
        required: String,
    },
    RequiredBy {
        plugin: String,
        dependents: Vec<String>,
    },
    CatalogDownloadFailed {
        catalog_name: String,
        error: String,
    },
}

impl From<&Issue> for IssueView {
    fn from(issue: &Issue) -> Self {
        match issue.clone() {
            Issue::MissingRequirement { plugin, requires } => {
                IssueView::MissingRequirement { plugin, requires }
            }
            Issue::AmbiguousRequirement {
                plugin,
                requires,
                candidates,
            } => IssueView::AmbiguousRequirement {
                plugin,
                requires,
                candidates,
            },
            Issue::IdentityMismatch {
                expected,
                catalog_name,
                actual,
            } => IssueView::IdentityMismatch {
                expected,
                catalog_name,
                actual,
            },
            Issue::Conflict { a, b } => IssueView::Conflict { a, b },
            Issue::GameTooOld {
                plugin,
                required,
                actual,
            } => IssueView::GameTooOld {
                plugin,
                required,
                actual,
            },
            Issue::GameVersionUnknown { plugin, required } => {
                IssueView::GameVersionUnknown { plugin, required }
            }
            Issue::RequiredBy { plugin, dependents } => {
                IssueView::RequiredBy { plugin, dependents }
            }
            Issue::CatalogDownloadFailed {
                catalog_name,
                error,
            } => IssueView::CatalogDownloadFailed {
                catalog_name,
                error,
            },
        }
    }
}

pub fn issue_views(issues: &[Issue]) -> Vec<IssueView> {
    issues.iter().map(IssueView::from).collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum NoteView {
    OptionalAvailable {
        plugin: String,
        optional: String,
        installed: bool,
    },
}

impl From<&Note> for NoteView {
    fn from(note: &Note) -> Self {
        match note.clone() {
            Note::OptionalAvailable {
                plugin,
                optional,
                installed,
            } => NoteView::OptionalAvailable {
                plugin,
                optional,
                installed,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PlanKind {
    Install,
    Update,
    Enable,
    Disable,
    Uninstall,
    ApplyProfile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum StepView {
    Install {
        catalog_name: String,
        identity: String,
        folder: String,
        version: String,
        /// Pulled in by a `requires`, rather than the plugin the user asked for.
        dependency: bool,
    },
    Update {
        catalog_name: String,
        identity: String,
        folder: String,
        from: String,
        to: String,
    },
    Enable {
        identity: String,
    },
    Disable {
        identity: String,
    },
    Uninstall {
        folder: String,
        identity: String,
    },
}

/// A way out of a `MissingRequirement`/`AmbiguousRequirement`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RequirementFixView {
    pub requires: String,
    /// It's installed but disabled: enable it (identity). Excludes the other two.
    pub enable: Option<String>,
    /// One catalog entry to install. Excludes `candidates`.
    pub install: Option<String>,
    /// Several catalog entries match; the user picks.
    pub candidates: Vec<String>,
}

/// A pending plan, held by the backend until it's committed, discarded or superseded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanView {
    pub plan_id: u32,
    pub kind: PlanKind,
    /// What the user asked for: a catalog name, identity, folder or profile name.
    pub target: String,
    pub steps: Vec<StepView>,
    /// Blocking: every one must be resolved or explicitly overridden (decision C).
    pub issues: Vec<IssueView>,
    /// Never blocking.
    pub notes: Vec<NoteView>,
    pub fixes: Vec<RequirementFixView>,
    /// Identities `resolve_conflict` can disable to clear a `conflict` issue (install plans).
    pub resolvable_conflicts: Vec<String>,
    /// Profile plans: entries the profile enables that aren't installed.
    pub missing: Vec<MissingView>,
    /// The installed game's version the checks ran against.
    pub game_version: Option<String>,
    /// Uninstall plans for a plugin the manager didn't install (decision B).
    pub unmanaged_target: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommitView {
    /// Identities of the plugins installed (or replaced, for an update).
    pub installed: Vec<String>,
    pub enabled: Vec<String>,
    pub disabled: Vec<String>,
    pub backup: Option<String>,
    /// Couldn't tell whether the game is running; warn that it may overwrite `plugins.txt`.
    pub game_detection_failed: bool,
    /// Old copies a replace couldn't delete; the install itself succeeded.
    pub leftovers: Vec<String>,
}

impl From<&CommitReport> for CommitView {
    fn from(report: &CommitReport) -> Self {
        CommitView {
            installed: report
                .installed
                .iter()
                .map(|i| i.identity().to_string())
                .collect(),
            enabled: report.enabled.clone(),
            disabled: report.disabled.clone(),
            backup: report.backup.as_ref().map(|p| p.display().to_string()),
            game_detection_failed: report.game_detection_failed,
            leftovers: report
                .installed
                .iter()
                .filter_map(|i| i.leftover.as_ref())
                .map(|p| p.display().to_string())
                .collect(),
        }
    }
}

/// Emitted as the `download-progress` event while a plan downloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DownloadProgress {
    pub plan_id: u32,
    pub catalog_name: String,
    #[ts(type = "number")]
    pub received: u64,
    #[ts(type = "number | null")]
    pub total: Option<u64>,
}
