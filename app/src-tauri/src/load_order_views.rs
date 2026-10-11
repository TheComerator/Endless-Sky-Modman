//! What the optional "Load order" page sends to the frontend (see `esmm_core::load_order`).
//!
//! Same conventions as `views.rs`: plain serializable data, camelCase on the wire, exported to
//! `app/src/bindings/` by ts-rs when `cargo test` runs. Deleting the feature deletes this file.

use esmm_core::load_order::{ChangeKind, ChangePlan, ExcludeReason, Violation, WinnerChange};
use esmm_core::overlap::OverlapItem;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LoadOrderExcludeReason {
    /// Its `plugin.txt` has no `name`, so renaming its folder would change its identity.
    NoPluginName,
    /// It was put in `plugins/` by hand; the manager only renames what it installed.
    NotInstalledByManager,
}

impl From<ExcludeReason> for LoadOrderExcludeReason {
    fn from(r: ExcludeReason) -> Self {
        match r {
            ExcludeReason::NoPluginName => LoadOrderExcludeReason::NoPluginName,
            ExcludeReason::NotInstalledByManager => LoadOrderExcludeReason::NotInstalledByManager,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadOrderPluginView {
    /// The folder name on disk right now; the key the other commands take.
    pub folder: String,
    pub identity: String,
    pub original_folder: String,
    /// The number in its folder prefix, if it has one.
    pub number: Option<u32>,
    pub enabled: bool,
    pub requires: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadOrderExcludedView {
    pub folder: String,
    pub identity: String,
    pub reasons: Vec<LoadOrderExcludeReason>,
    /// Still carries a number prefix from earlier; "Restore original names" fixes it.
    pub still_renamed: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadOrderState {
    /// The Settings switch.
    pub enabled: bool,
    /// Plugins that can be ordered, in the order the game loads them now. Empty when the
    /// switch is off.
    pub plugins: Vec<LoadOrderPluginView>,
    pub excluded: Vec<LoadOrderExcludedView>,
    /// How many folders currently carry a prefix this feature added.
    pub renamed_count: u32,
}

impl LoadOrderState {
    /// Nothing to list: the switch is off, or there is no install to read.
    pub fn empty(enabled: bool, renamed_count: u32) -> Self {
        LoadOrderState {
            enabled,
            plugins: Vec::new(),
            excluded: Vec::new(),
            renamed_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadOrderSwitch {
    pub enabled: bool,
    /// Folders still renamed; when switching off and this is not 0 the UI offers to restore.
    pub renamed_count: u32,
}

/// A plugin that would load before one of the plugins it `requires`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadOrderDependencyNote {
    pub plugin: String,
    pub requires: String,
}

impl From<&Violation> for LoadOrderDependencyNote {
    fn from(v: &Violation) -> Self {
        LoadOrderDependencyNote {
            plugin: v.plugin.clone(),
            requires: v.requires.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadOrderSuggestion {
    /// Folders, first loaded first.
    pub order: Vec<String>,
    /// The dependency problems this order fixes.
    pub fixed: Vec<LoadOrderDependencyNote>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadOrderWinnerChange {
    /// Plain text such as `ship "Hornet"` or `image ship/hornet`.
    pub item: String,
    pub was: Option<String>,
    pub now: Option<String>,
    pub overridden: Vec<String>,
    pub reason: String,
}

fn item_text(item: &OverlapItem) -> String {
    match item {
        OverlapItem::Object { kind, name } => format!("{kind} \"{name}\""),
        OverlapItem::Image(name) => format!("image {name}"),
        OverlapItem::Sound(name) => format!("sound {name}"),
    }
}

impl From<&WinnerChange> for LoadOrderWinnerChange {
    fn from(c: &WinnerChange) -> Self {
        LoadOrderWinnerChange {
            item: item_text(&c.item),
            was: c.was.clone(),
            now: c.now.clone(),
            overridden: c.overridden.clone(),
            reason: c.reason.clone(),
        }
    }
}

/// What reordering would do, for the page's live banner (not a commitment to anything).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadOrderPreview {
    pub dependency_notes: Vec<LoadOrderDependencyNote>,
    /// Overlaps whose winner would change.
    pub changes: Vec<LoadOrderWinnerChange>,
    pub rename_count: u32,
    /// Why this order can't be applied (a name clash, a path too long, ...), if so.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadOrderRename {
    pub from: String,
    pub to: String,
    pub identity: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LoadOrderPlanKind {
    Apply,
    Restore,
}

impl From<ChangeKind> for LoadOrderPlanKind {
    fn from(k: ChangeKind) -> Self {
        match k {
            ChangeKind::Apply => LoadOrderPlanKind::Apply,
            ChangeKind::Restore => LoadOrderPlanKind::Restore,
        }
    }
}

/// A plugin whose name (as the game knows it) would change, because it has no `name` in its
/// plugin.txt and so is known by its folder name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadOrderIdentityChange {
    pub from: String,
    pub to: String,
}

/// Exactly which folders a commit would rename, for the review dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadOrderPlanView {
    pub kind: LoadOrderPlanKind,
    /// Passed back to the commit; it is refused if the plan would be different by then.
    pub token: String,
    pub renames: Vec<LoadOrderRename>,
    /// Plugins outside the order whose names make them load before or among the numbered ones.
    pub loads_among: Vec<String>,
    pub changes: Vec<LoadOrderWinnerChange>,
    /// Plugins that would become a different plugin to the game (restoring only).
    pub identity_changes: Vec<LoadOrderIdentityChange>,
}

impl LoadOrderPlanView {
    pub fn new(plan: &ChangePlan, token: String, changes: &[WinnerChange]) -> Self {
        LoadOrderPlanView {
            kind: plan.kind.into(),
            token,
            renames: plan
                .renames
                .iter()
                .map(|r| LoadOrderRename {
                    from: r.from.clone(),
                    to: r.to.clone(),
                    identity: r.identity.clone(),
                })
                .collect(),
            loads_among: plan.loads_among.clone(),
            changes: changes.iter().map(LoadOrderWinnerChange::from).collect(),
            identity_changes: plan
                .identity_changes
                .iter()
                .map(|(from, to)| LoadOrderIdentityChange {
                    from: from.clone(),
                    to: to.clone(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadOrderCommitView {
    pub renamed: u32,
    pub backup: Option<String>,
}
