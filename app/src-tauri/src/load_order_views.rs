//! What the optional "Load order" page sends to the frontend (see `esmm_core::load_order`).
//!
//! Same conventions as `views.rs`: plain serializable data, camelCase on the wire, exported to
//! `app/src/bindings/` by ts-rs when `cargo test` runs. Deleting the feature deletes this file.

use std::collections::HashSet;

use esmm_core::load_order::{ChangeKind, ChangePlan, ExcludeReason, Violation, WinnerChange};
use esmm_core::overlap::OverlapItem;
use esmm_core::plugin_naming::{LinePosition, NamingPlan, Relation};
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LoadOrderExcludeReason {
    /// Its `plugin.txt` has no `name`, so renaming its folder would change its identity.
    NoPluginName,
    /// The manager has no record of it (put in by hand and not linked to the catalog).
    NotLinkedToCatalog,
    /// Its folder name isn't one the manager renames (odd characters, or too long).
    UnsupportedFolderName,
}

impl From<ExcludeReason> for LoadOrderExcludeReason {
    fn from(r: ExcludeReason) -> Self {
        match r {
            ExcludeReason::NoPluginName => LoadOrderExcludeReason::NoPluginName,
            ExcludeReason::NotLinkedToCatalog => LoadOrderExcludeReason::NotLinkedToCatalog,
            ExcludeReason::UnsupportedFolderName => LoadOrderExcludeReason::UnsupportedFolderName,
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
    /// Its only problem is that its `plugin.txt` has no name (and the manager has a record of
    /// it), so the page offers "Give it a name…" (`load_order_plan_name`).
    pub can_name: bool,
    /// When a name is what it lacks but it can't be given one here, why not (its folder still
    /// has a number, it is a link to a folder elsewhere, its plugin.txt has an empty `name`
    /// line...), shown in place of the button.
    pub name_hint: Option<String>,
}

/// How another plugin's `plugin.txt` mentions the plugin being named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LoadOrderNameRelation {
    Requires,
    Optional,
    Conflicts,
}

impl From<Relation> for LoadOrderNameRelation {
    fn from(r: Relation) -> Self {
        match r {
            Relation::Requires => LoadOrderNameRelation::Requires,
            Relation::Optional => LoadOrderNameRelation::Optional,
            Relation::Conflicts => LoadOrderNameRelation::Conflicts,
        }
    }
}

/// Another installed plugin that mentions the plugin's old name; after a rename it no longer
/// matches, and the manager can't change that plugin's files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadOrderNameDependent {
    /// That plugin's own name.
    pub plugin: String,
    pub relation: LoadOrderNameRelation,
}

/// Exactly what "Give it a name" would write and change, for its dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadOrderNamePlanView {
    pub folder: String,
    /// The name that would be written, with surrounding spaces already trimmed.
    pub name: String,
    /// What the game calls the plugin now: its folder name.
    pub old_identity: String,
    /// The exact line added to `plugin.txt`, e.g. `name "Foo"`.
    pub line: String,
    /// The line goes at the end of the file instead of the top (its first line is indented).
    pub line_at_end: bool,
    /// There is no `plugin.txt` yet; one is created holding just `line`.
    pub creates_file: bool,
    /// Where a copy of `plugin.txt` as it was (or a note that there was none) is kept.
    pub backup: String,
    /// The name differs from the folder name, so the plugin's identity changes.
    pub identity_changes: bool,
    /// Whether it is switched on now; the setting moves to the new name unchanged.
    pub enabled: bool,
    /// Where `plugins.txt` is copied first (`plugins.txt.esmm-naming-bak`), when a new name
    /// changes it.
    pub plugins_txt_backup: Option<String>,
    /// Profiles that list it under its old name; they are updated to the new name.
    pub profiles: Vec<String>,
    /// Other plugins whose dependencies mention the old name.
    pub dependents: Vec<LoadOrderNameDependent>,
    /// Passed back to the commit; it is refused if the plan would be different by then.
    pub token: String,
}

impl LoadOrderNamePlanView {
    pub fn new(plan: &NamingPlan, token: String) -> Self {
        LoadOrderNamePlanView {
            folder: plan.folder.clone(),
            name: plan.name.clone(),
            old_identity: plan.old_identity.clone(),
            line: plan.line.clone(),
            line_at_end: plan.position == LinePosition::End,
            creates_file: plan.creates_file(),
            backup: plan.backup_path.display().to_string(),
            identity_changes: plan.identity_changes(),
            enabled: plan.enabled,
            plugins_txt_backup: plan
                .plugin_states_backup
                .as_ref()
                .map(|p| p.display().to_string()),
            profiles: plan.profiles.clone(),
            dependents: plan
                .dependents
                .iter()
                .map(|d| LoadOrderNameDependent {
                    plugin: d.plugin.clone(),
                    relation: d.relation.into(),
                })
                .collect(),
            token,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadOrderNameCommitView {
    /// The name now in the plugin's `plugin.txt`.
    pub name: String,
    /// Where the copy of the old `plugin.txt` (or the note) was saved.
    pub backup: String,
    pub identity_changed: bool,
    /// Where `plugins.txt` was copied before it changed, if it did.
    pub plugins_txt_backup: Option<String>,
    /// The game's process list couldn't be read while `plugins.txt` was written.
    pub game_detection_failed: bool,
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
    /// While an earlier renaming or naming is unfinished (nothing else can be changed until it
    /// is), what to tell the user: what it is, why, and what they can do. Shown as a banner.
    pub unfinished: Option<String>,
}

impl LoadOrderState {
    /// Nothing to list: the switch is off, or there is no install to read.
    pub fn empty(enabled: bool, renamed_count: u32) -> Self {
        LoadOrderState {
            enabled,
            plugins: Vec::new(),
            excluded: Vec::new(),
            renamed_count,
            unfinished: None,
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
    /// The user put this plugin in `plugins/` by hand (the manager only linked it to the
    /// catalog), so it is the folder they placed that is being renamed.
    pub by_hand: bool,
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
    /// `by_hand`: folders (by current name) that the user placed themselves.
    pub fn new(
        plan: &ChangePlan,
        token: String,
        changes: &[WinnerChange],
        by_hand: &HashSet<String>,
    ) -> Self {
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
                    by_hand: by_hand.contains(&r.from),
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
