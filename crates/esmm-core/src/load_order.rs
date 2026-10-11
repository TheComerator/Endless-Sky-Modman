//! Optional plugin load-order management (a stop-gap until/unless the game supports it).
//!
//! # What it does, and why
//!
//! The game loads plugin folders in sorted order of their folder names, and a plugin loaded
//! later wins wherever two plugins set the same field (`Files::ListDirectories` sorts the paths,
//! `GameData::LoadSources` loads them in that order). The folder name is chosen by whoever
//! packaged the plugin, so a user has no direct say. This module lets the user choose: it gives
//! each *eligible* plugin folder a fixed-width numeric prefix (`010-Name`, `020-Name`, ...), which
//! sorts before letters, and renames folders to apply the order the user picked.
//!
//! # Rules this module keeps
//!
//! * **Identity.** A plugin's identity is the `name` in its `plugin.txt`, else its folder name.
//!   Renaming a folder only keeps the identity if `plugin.txt` names the plugin, so a plugin
//!   without a name is never renamed ([`ExcludeReason::NoPluginName`]). This module never
//!   writes inside a plugin folder. The page can offer such a plugin a name instead (the user
//!   picks its folder name or types one); that separate step, `crate::plugin_naming`, adds one
//!   `name` line to its `plugin.txt`, after which it is ordered like any other plugin.
//! * **Only plugins the manager has a record for** are renamed: ones it installed, and ones put in
//!   `plugins/` by hand that it linked to the catalog (adopted), since a record is where the
//!   original folder name is remembered so a restore is possible. A plugin it knows nothing about
//!   is never touched ([`ExcludeReason::NotLinkedToCatalog`]), and neither is one whose folder
//!   name isn't a plain, short-enough name ([`ExcludeReason::UnsupportedFolderName`]).
//! * **Records.** An install record's `folder` is always the name on disk right now (and the key);
//!   `original_folder` remembers the name before the first rename. Profiles and `plugins.txt` use
//!   identities, so they keep working untouched.
//! * **Safety.** Renames refuse to run while the game is running; the intended renames are
//!   written to a journal *before* the first one and the journal is removed after the records are
//!   updated; [`LoadOrderBackend::recover`] finishes or undoes an interrupted run; nothing is
//!   ever renamed onto an existing folder; `plugins.txt` is backed up first.
//! * **Reversible.** [`LoadOrderBackend::plan_restore`] puts every original name back.
//!
//! # How to remove this feature
//!
//! The optional feature has two parts: **the load order** (this file: numbering folders, the
//! Load order page) and **giving a plugin a name** (`plugin_naming.rs`, offered on that page).
//! Callers only use this module through [`LoadOrderBackend`] (and the plain data types next to
//! it). Run "Restore original names" first if any plugin is still renamed.
//!
//! **Before removing the naming part, look in `records.json`.** If any record has an
//! `added_name`, the manager wrote a name into that plugin's `plugin.txt`, and every update
//! writes it back; without the naming part's core (list N1 below) the next update would
//! silently drop the name, and with it the plugin's on/off setting and profile entries. So:
//! while any record has an `added_name`, remove only the load order (case 1). The names stay in
//! the plugins' `plugin.txt` files; copies of the files as they were are in
//! `plugin-txt-backups/` beside `records.json`.
//!
//! **Case 1, remove the load order only** (the naming part's core keeps working for updates,
//! without any page): delete list L.
//! **Case 2, remove the naming part only** (keep the load order): delete lists N1 and N2.
//! **Case 3, remove both:** delete lists L and N1 (N2 is inside L's files).
//!
//! **List L, the load order and its page** (most lines are marked `load order` or
//! `load_order` where they appear):
//! * Files: `crates/esmm-core/src/load_order.rs` (this file), `crates/esmm-core/tests/load_order.rs`,
//!   `crates/esmm-core/tests/load_order_naming.rs`, `app/src-tauri/src/shell/load_order.rs`,
//!   `app/src-tauri/src/load_order_views.rs`, `app/src/views/LoadOrderView.tsx`,
//!   `app/src/hooks/useLoadOrder.ts`, `app/src/loadOrder.ts`, `app/src/loadOrder.test.ts`, and
//!   the generated `app/src/bindings/LoadOrder*.ts` (`cargo test` no longer writes them).
//! * `crates/esmm-core/src/lib.rs`: `pub mod load_order;`
//! * `app/src-tauri/src/lib.rs`: `mod load_order_views;` and every `commands::load_order_*`
//!   entry
//! * `app/src-tauri/src/shell.rs`: the `// load order` comment and `mod load_order;`, and the
//!   `recover_interrupted` call (with its comment) in `snapshot`
//! * `app/src-tauri/src/commands.rs`: the `use crate::load_order_views::*;` line and the "Load
//!   order" block of command wrappers at the end of the file
//! * `app/src-tauri/src/settings.rs`: the `manage_load_order` field, the test
//!   `load_order_management_is_off_unless_the_settings_file_says_otherwise`, and the
//!   `manage_load_order: true,` line in `settings_round_trip_and_tolerate_corruption`. An old
//!   `settings.json` that still contains the setting is simply ignored.
//! * `app/src/App.tsx`: the imports, `"loadOrder"` in `type Tab`, the `useLoadOrder` line, the
//!   tab-list entry, the effect that leaves the page when the switch is turned off, the
//!   `loadOrder` page render and the `loadOrder` prop passed to `SettingsView`
//! * `app/src/views/SettingsView.tsx`: the imports, the `loadOrder` prop (and its type) and
//!   `<LoadOrderSettings .../>`
//! * `app/src/api.ts`: the `LoadOrder*` type imports and every `loadOrder*` entry
//! * `app/src/styles.css`: the three "Load order" blocks at the end
//!
//! **List N1, the naming part's core** (all of it is needed for updates while any record has an
//! `added_name`):
//! * Files: `crates/esmm-core/src/plugin_naming.rs`, `crates/esmm-core/tests/plugin_naming.rs`
//! * `crates/esmm-core/src/lib.rs`: `pub mod plugin_naming;`
//! * `crates/esmm-core/src/manager.rs` (every piece is commented "plugin naming"): the
//!   `use crate::plugin_naming::...` line; `expect_added_name` and its two calls (in
//!   `stage_for_install`, which then returns no issue, and in `plan_update`, with
//!   `naming_issue`); `decide_added_names`, `decide_added_name`, `write_added_names`,
//!   `write_added_name` and every call to them; the `name_to_write` and `added_name` fields of
//!   `StagedPlugin` (records are then built with `added_name: None`)
//! * `crates/esmm-core/src/resolve.rs`: `Issue::AddedNameCannotBeKept`;
//!   `app/src-tauri/src/views.rs`: `IssueView::AddedNameCannotBeKept` and its line in
//!   `From<&Issue>`; `app/src/format.ts`: the `addedNameCannotBeKept` case;
//!   `app/src/format.test.ts`: its two lines
//! * `crates/esmm-core/src/load_order.rs`, in case 2 only: the `plugin_naming` journal in
//!   `journal_pending`, the second half of the test `a_pending_journal_blocks_new_changes`, and
//!   the `plugin_naming` lines of the test `the_naming_step_and_the_manager_agree_with_this_module`
//! * `app/src-tauri/src/shell.rs`: `use esmm_core::plugin_naming;`, the
//!   `recover_interrupted_naming` call (with its comment) in `snapshot` and the function itself,
//!   and the `naming_paths` function
//!
//! **List N2, the naming part's page** (case 2 only; in case 3 it goes with list L's files):
//! * `app/src-tauri/src/shell/load_order.rs`: `naming_err`, `name_token`, `name_choice`,
//!   `naming_offer`, `require_nameable`, `load_order_plan_name`, `load_order_commit_name`, the
//!   `naming` variable and the `can_name`/`name_hint` fields in `load_order_state`, the
//!   `plugin_naming`/`naming_paths`/`ExcludeReason` imports, and the tests from
//!   `--- "Give it a name"` on plus `naming_commands_through_ipc`
//! * `app/src-tauri/src/load_order_views.rs`: `can_name`, `name_hint`, and the
//!   `LoadOrderName*` types with the `plugin_naming` import; `app/src/bindings/LoadOrderName*.ts`
//! * `app/src-tauri/src/commands.rs` and `app/src-tauri/src/lib.rs`: `load_order_plan_name` and
//!   `load_order_commit_name`
//! * `app/src/api.ts`: `loadOrderPlanName`, `loadOrderCommitName` and their two type imports
//! * `app/src/views/LoadOrderView.tsx`: `NameDialog`, the `naming` state, `onNamed`, the "Give it
//!   a name" button and hint, the `<NameDialog>` render, the `ReactNode` and `LoadOrderName*`
//!   imports and the `describeName*` imports, and `e.canName` passed to `describeExclusion`
//! * `app/src/loadOrder.ts`: `describeNameDependent`, `describeNameConsequences`,
//!   `describeNameWrite`, their imports, and the `canName` parameter of `describeExclusion`;
//!   `app/src/loadOrder.test.ts`: the "naming a plugin" tests, `namePlan`, and the `canName` lines
//! * `app/src/styles.css`: the `"Give it a name" dialog` block
//!
//! Then run `cargo fmt` (deleting lines leaves stray blank lines) and, in cases 1 and 3, delete
//! the empty `app/src-tauri/src/shell/` folder. Before the naming part existed, list L was
//! checked in a throwaway copy (`cargo fmt --check`, `cargo clippy --all-targets`,
//! `cargo test --workspace`, `npm run build` and `npm test` all passed); see `CLAUDE.md`
//! (decision M) for which of the three cases have been checked since.
//!
//! Documentation is not code and nothing breaks if it stays, but if the feature is removed for
//! good, also deal with decision M and roadmap item 1d in `CLAUDE.md`, the README bullets,
//! `docs/proposals/endless-sky-load-order-request.md` and
//! `project-memory/prompts/load-order-cloud-session.md`.
//!
//! These stay in every case and are harmless without the feature: the `original_folder` and
//! `added_name` fields on `InstallRecord`, `files::plugin_folders_lock`, `manager::begin_change`
//! and `manager::UNFINISHED_CHANGE_JOURNALS` (a journal that never exists blocks nothing), and
//! the "update in place" and "install into the folder it is already in" rules in `manager.rs`.
//!
//! A future native mechanism in the game would be another implementation of
//! [`LoadOrderBackend`]; callers do not need to change.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::MutexGuard;

use serde::{Deserialize, Serialize};

use crate::files;
use crate::game_state::{self, GameProcess};
use crate::manager::InstalledPlugin;
use crate::overlap::{self, OverlapItem, PluginEntry, PluginIndex};
use crate::plugin_meta::PluginMeta;
use crate::records::{self, InstallRecord};

/// The journal sits next to `records.json`.
pub const JOURNAL_FILE: &str = "load-order-journal.json";
/// Where `plugins.txt` is copied before a change. Deliberately not the file the toggle code
/// uses (`plugins.txt.esmm-bak`): a load-order change never writes `plugins.txt`, so it must
/// not replace the only backup of what the game's own list used to say.
pub const BACKUP_FILE: &str = "plugins.txt.esmm-load-order-bak";

/// Prefixes are three digits (`010-`), so there are at most 999 numbers.
const MAX_NUMBER: u32 = 999;
/// Numbers are handed out in steps of 10 (leaving room to insert) while that fits.
const STEP: u32 = 10;
/// Longest folder name, in bytes, that every supported filesystem accepts.
const MAX_FOLDER_NAME_BYTES: usize = 255;
/// Windows' `MAX_PATH` (260) counts the terminating NUL.
const WINDOWS_MAX_PATH_CHARS: usize = 259;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum LoadOrderError {
    #[error("Endless Sky is running; close it first")]
    GameRunning,
    #[error(
        "An earlier change to the plugin folders was interrupted and is still being tidied up. The manager finishes or undoes it by itself the next time it reads your plugins; try again in a moment."
    )]
    JournalPending,
    #[error("The plugin list changed since this order was made. Reload the Load order page.")]
    OrderMismatch,
    #[error(
        "There are {0} plugins to order, but only {MAX_NUMBER} numbers fit in a three-digit prefix."
    )]
    TooMany(usize),
    #[error(
        "{name:?} would be too long a folder name ({length} bytes, the limit is {MAX_FOLDER_NAME_BYTES})."
    )]
    NameTooLong { name: String, length: usize },
    #[error(
        "Can't rename to {name:?}: a folder or file called {existing:?} is already in plugins/."
    )]
    Collision { name: String, existing: String },
    #[error(
        "Renaming {folder:?} would make its longest file path {length} characters, over the {limit} that Windows allows."
    )]
    PathTooLong {
        folder: String,
        length: usize,
        limit: usize,
    },
    #[error("The plugin folder {0:?} is no longer in plugins/.")]
    Missing(String),
    #[error("The folder {0:?} is no longer the plugin it was when this change was planned.")]
    Changed(String),
    #[error(
        "Couldn't rename {from:?} to {to:?}: {error}. {}",
        if *.rolled_back { "Everything already renamed was put back." } else { "Some folders could not be put back, or the manager could not tidy up after itself; restart the manager to finish or undo the change." }
    )]
    RenameFailed {
        from: String,
        to: String,
        error: String,
        rolled_back: bool,
    },
    #[error(
        "The reordering journal is unreadable ({0}). It was left in place; nothing was changed."
    )]
    Journal(String),
    #[error(
        "Folders could not be put into a consistent state ({0}). The journal was left in place."
    )]
    Unrecoverable(String),
    #[error("Couldn't update the install records: {0}")]
    Records(String),
    #[error("{0}")]
    Io(String),
}

impl From<io::Error> for LoadOrderError {
    fn from(e: io::Error) -> Self {
        LoadOrderError::Io(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// The surface callers use
// ---------------------------------------------------------------------------

/// Where things are, for one game config dir.
#[derive(Debug, Clone)]
pub struct Paths<'a> {
    pub config_dir: &'a Path,
    /// The install's `records.json`; the journal is kept beside it.
    pub records_path: &'a Path,
    /// The longest full path (in characters) a renamed plugin's files may reach, if the
    /// platform has such a limit. `Paths::new` sets it on Windows only.
    pub max_path_chars: Option<usize>,
}

impl<'a> Paths<'a> {
    pub fn new(config_dir: &'a Path, records_path: &'a Path) -> Self {
        Paths {
            config_dir,
            records_path,
            max_path_chars: cfg!(windows).then_some(WINDOWS_MAX_PATH_CHARS),
        }
    }

    fn plugins_dir(&self) -> PathBuf {
        crate::install::plugins_dir(self.config_dir)
    }

    fn journal_path(&self) -> PathBuf {
        self.records_path.with_file_name(JOURNAL_FILE)
    }
}

/// Why a plugin can't be part of the ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExcludeReason {
    /// Its `plugin.txt` has no `name` (or there is no `plugin.txt`), so the game uses the folder
    /// name as its identity: renaming the folder would make the game see a different plugin.
    NoPluginName,
    /// The manager has no record of it: it was put in `plugins/` by hand and isn't linked to a
    /// catalog plugin, so there is nowhere to remember its original folder name and a restore
    /// would be impossible.
    NotLinkedToCatalog,
    /// Its folder name has characters the manager doesn't rename (a path separator, a trailing
    /// dot or space, a name Windows reserves) or is too long to take a three-digit prefix.
    UnsupportedFolderName,
}

/// A plugin that can be ordered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderedPlugin {
    /// The name on disk now; also this plugin's key everywhere in this module.
    pub folder: String,
    pub identity: String,
    /// The name it had before any renaming (equal to `folder` if never renamed).
    pub original_folder: String,
    /// The number in its current prefix, if it has one this module put there.
    pub number: Option<u32>,
    /// Identities this plugin `requires`, from its `plugin.txt`.
    pub requires: Vec<String>,
}

/// A plugin that is left out of the ordering, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Excluded {
    pub folder: String,
    pub identity: String,
    pub reasons: Vec<ExcludeReason>,
    /// It still carries a number prefix from earlier (for example an update removed its name);
    /// "Restore original names" will put it right.
    pub still_renamed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layout {
    /// Eligible plugins in the order the game loads them right now.
    pub ordered: Vec<OrderedPlugin>,
    pub excluded: Vec<Excluded>,
    /// Folders currently renamed (their record has an `original_folder`), eligible or not.
    pub restorable: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Apply,
    Restore,
}

/// One folder rename, as planned and as written to the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rename {
    pub from: String,
    pub to: String,
    pub identity: String,
    /// The record's `original_folder` before and after this rename, so the record can be
    /// brought in line with whichever folder name is on disk.
    pub from_original: Option<String>,
    pub to_original: Option<String>,
}

/// Exactly what a commit would do. Planning never changes anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePlan {
    pub kind: ChangeKind,
    pub renames: Vec<Rename>,
    /// Plugins that are not part of the ordering but whose names make them load before or
    /// among the numbered ones (for example a hand-installed `050-Custom`, or a name that
    /// starts with a symbol).
    pub loads_among: Vec<String>,
    /// Plugins whose identity would change: a plugin without a `name` in its `plugin.txt` is
    /// known to the game by its folder name, so putting an original folder name back changes
    /// it (`(old, new)`). Only a restore can do this.
    pub identity_changes: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitOutcome {
    pub renamed: usize,
    pub backup: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    /// There was no interrupted run.
    Nothing,
    /// The interrupted run was carried through to the end.
    Finished { renames: usize },
    /// It could not be carried through, so it was undone.
    RolledBack { renames: usize, reason: String },
}

/// How an order is put into effect. The folder-prefix implementation is the only one today;
/// a native mechanism in the game would be another.
pub trait LoadOrderBackend {
    fn name(&self) -> &'static str;

    /// The plugins that can be ordered (in current load order) and those that can't.
    fn layout(&self, installed: &[InstalledPlugin]) -> Layout;

    /// What it would take to make `wanted` (the folders from [`Layout::ordered`], first loaded
    /// first) the load order.
    fn plan_apply(
        &self,
        paths: &Paths,
        installed: &[InstalledPlugin],
        wanted: &[String],
    ) -> Result<ChangePlan, LoadOrderError>;

    /// What it would take to put every renamed folder back to its original name.
    fn plan_restore(
        &self,
        paths: &Paths,
        installed: &[InstalledPlugin],
    ) -> Result<ChangePlan, LoadOrderError>;

    /// Carries a plan out. Refuses while the game runs.
    fn commit(
        &self,
        paths: &Paths,
        plan: &ChangePlan,
        game: GameProcess,
    ) -> Result<CommitOutcome, LoadOrderError>;

    /// Finishes or undoes a run that was interrupted (a crash, a power cut). Call it before
    /// the plugin folders are read.
    fn recover(&self, paths: &Paths, game: GameProcess) -> Result<Recovery, LoadOrderError>;

    /// The name the game sorts a plugin by after `plan` (its folder name, for this backend).
    fn sort_key_after(&self, plan: &ChangePlan, folder: &str) -> String {
        plan.renames
            .iter()
            .find(|r| r.from == folder)
            .map_or_else(|| folder.to_string(), |r| r.to.clone())
    }
}

/// The only backend today: numeric folder-name prefixes.
pub fn default_backend() -> impl LoadOrderBackend {
    FolderPrefixBackend
}

// ---------------------------------------------------------------------------
// The game's order
// ---------------------------------------------------------------------------

/// How the game orders plugin folders: `Files::ListDirectories` sorts the paths with `std::sort`,
/// which compares them character by character and case-sensitively, so `-`, then digits, then
/// uppercase, then `_`, then lowercase. (On Windows the game compares UTF-16 units, which only
/// differs from this for characters outside the Basic Multilingual Plane; unconfirmed there.)
pub fn game_order(a: &str, b: &str) -> Ordering {
    a.as_bytes().cmp(b.as_bytes())
}

fn numbered_name(number: u32, original: &str) -> String {
    format!("{number:03}-{original}")
}

/// The number if `folder` is exactly `NNN-<original>`.
fn parse_number(folder: &str, original: &str) -> Option<u32> {
    let rest = folder.strip_suffix(original)?;
    let digits = rest.strip_suffix('-')?;
    (digits.len() == 3 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
        .filter(|n| (1..=MAX_NUMBER).contains(n))
}

fn is_blank(name: &Option<String>) -> bool {
    name.as_deref().is_none_or(|n| n.trim().is_empty())
}

fn exclusion_reasons(plugin: &InstalledPlugin) -> Vec<ExcludeReason> {
    let mut reasons = Vec::new();
    if plugin.record.is_none() {
        reasons.push(ExcludeReason::NotLinkedToCatalog);
    }
    if is_blank(&plugin.meta.name) {
        reasons.push(ExcludeReason::NoPluginName);
    }
    // A hand-named folder can be anything; the renamed form must be a plain name that fits.
    let original = original_of(plugin);
    if !is_safe_name(&original) || original.len() + 4 > MAX_FOLDER_NAME_BYTES {
        reasons.push(ExcludeReason::UnsupportedFolderName);
    }
    reasons
}

fn original_of(plugin: &InstalledPlugin) -> String {
    plugin
        .record
        .as_ref()
        .and_then(|r| r.original_folder.clone())
        .unwrap_or_else(|| plugin.folder.clone())
}

// ---------------------------------------------------------------------------
// Numbering
// ---------------------------------------------------------------------------

/// Indices of a longest strictly increasing run among the numbers that exist.
fn longest_increasing(values: &[Option<u32>]) -> Vec<usize> {
    let n = values.len();
    let mut length = vec![0usize; n];
    let mut previous = vec![None; n];
    let mut best: Option<usize> = None;
    for i in 0..n {
        let Some(vi) = values[i] else { continue };
        length[i] = 1;
        for j in 0..i {
            if let Some(vj) = values[j]
                && vj < vi
                && length[j] + 1 > length[i]
            {
                length[i] = length[j] + 1;
                previous[i] = Some(j);
            }
        }
        if best.is_none_or(|b| length[i] > length[b]) {
            best = Some(i);
        }
    }
    let mut run = Vec::new();
    let mut cursor = best;
    while let Some(i) = cursor {
        run.push(i);
        cursor = previous[i];
    }
    run.reverse();
    run
}

/// Numbers for plugins in their wanted order, given each one's current number (if any).
///
/// A plugin whose number already fits the wanted order keeps it, so moving one plugin renames
/// only that plugin: a longest run of existing numbers that is already in order is kept, and the
/// rest are slotted into the gaps (continuing in steps of 10 at either end, spread evenly
/// between two kept numbers). When there is no room, or nothing has a number yet, everyone is
/// numbered afresh: 10, 20, 30, ... (or 1, 2, 3, ... for more than 99 plugins).
fn assign_numbers(current: &[Option<u32>]) -> Result<Vec<u32>, LoadOrderError> {
    // Only 1..=999 can be kept; anything else (a hand-edited record) is numbered afresh.
    let current: Vec<Option<u32>> = current
        .iter()
        .map(|n| n.filter(|n| (1..=MAX_NUMBER).contains(n)))
        .collect();
    let current = &current[..];
    let n = current.len();
    if n > MAX_NUMBER as usize {
        return Err(LoadOrderError::TooMany(n));
    }
    let fresh = || -> Vec<u32> {
        let step = if n * (STEP as usize) <= MAX_NUMBER as usize {
            STEP
        } else {
            1
        };
        (1..=n as u32).map(|i| i * step).collect()
    };
    let kept = longest_increasing(current);
    if kept.is_empty() {
        return Ok(fresh());
    }
    let mut out = vec![0u32; n];
    for &i in &kept {
        out[i] = current[i].expect("kept entries have numbers");
    }
    // The stretches of not-kept plugins between kept ones (and before the first / after the last).
    let mut edges: Vec<Option<usize>> = vec![None];
    edges.extend(kept.iter().map(|&i| Some(i)));
    edges.push(None);
    for pair in edges.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let first = a.map_or(0, |i| i + 1);
        let last = b.unwrap_or(n); // exclusive
        let k = (last - first) as u32;
        if k == 0 {
            continue;
        }
        let lo = a.map_or(0, |i| out[i]);
        let hi = b.map_or(MAX_NUMBER + 1, |i| out[i]);
        if hi.saturating_sub(lo + 1) < k {
            return Ok(fresh()); // no room: renumber everyone
        }
        for (j, slot) in (first..last).enumerate() {
            let j = j as u32 + 1;
            out[slot] = if b.is_none() && lo + STEP * k <= MAX_NUMBER {
                lo + STEP * j // after the last kept number, in steps of 10
            } else if a.is_none() && hi > STEP * k {
                hi - STEP * (k + 1 - j) // before the first kept number, in steps of 10
            } else {
                lo + (hi - lo) * j / (k + 1) // spread evenly
            };
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Dependencies
// ---------------------------------------------------------------------------

/// A plugin that loads before one of the plugins it `requires`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub plugin: String,
    pub requires: String,
}

/// Where `requires` is violated in `order` (folders, first loaded first). Requirements that
/// aren't in the ordering (not installed, or excluded) can't be placed, so they're ignored.
pub fn dependency_violations(layout: &Layout, order: &[String]) -> Vec<Violation> {
    let position: HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(i, f)| (f.as_str(), i))
        .collect();
    let by_identity: HashMap<&str, &OrderedPlugin> = layout
        .ordered
        .iter()
        .map(|p| (p.identity.as_str(), p))
        .collect();
    let mut found = Vec::new();
    for plugin in &layout.ordered {
        let Some(&at) = position.get(plugin.folder.as_str()) else {
            continue;
        };
        for required in &plugin.requires {
            let Some(other) = by_identity.get(required.as_str()) else {
                continue;
            };
            if other.folder != plugin.folder
                && position.get(other.folder.as_str()).is_some_and(|&p| p > at)
            {
                found.push(Violation {
                    plugin: plugin.identity.clone(),
                    requires: required.clone(),
                });
            }
        }
    }
    found
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// Folders, first loaded first.
    pub order: Vec<String>,
    /// The requirements the suggestion puts in front of the plugins that need them.
    pub fixed: Vec<Violation>,
}

/// The current order, changed as little as possible so every plugin loads after the plugins it
/// `requires`. Each plugin is placed right after its requirements; a cycle is broken wherever it
/// is first met. It is only a suggestion: the user can still change it.
pub fn suggest_order(layout: &Layout) -> Suggestion {
    let by_identity: HashMap<&str, &OrderedPlugin> = layout
        .ordered
        .iter()
        .map(|p| (p.identity.as_str(), p))
        .collect();
    let mut placed: HashSet<&str> = HashSet::new();
    let mut visiting: HashSet<&str> = HashSet::new();
    let mut order: Vec<String> = Vec::new();

    fn place<'a>(
        plugin: &'a OrderedPlugin,
        by_identity: &HashMap<&str, &'a OrderedPlugin>,
        placed: &mut HashSet<&'a str>,
        visiting: &mut HashSet<&'a str>,
        order: &mut Vec<String>,
    ) {
        if placed.contains(plugin.folder.as_str()) || !visiting.insert(plugin.folder.as_str()) {
            return;
        }
        let mut requires: Vec<&String> = plugin.requires.iter().collect();
        requires.sort();
        for required in requires {
            if let Some(other) = by_identity.get(required.as_str()) {
                place(other, by_identity, placed, visiting, order);
            }
        }
        visiting.remove(plugin.folder.as_str());
        placed.insert(plugin.folder.as_str());
        order.push(plugin.folder.clone());
    }

    for plugin in &layout.ordered {
        place(plugin, &by_identity, &mut placed, &mut visiting, &mut order);
    }
    let before: Vec<String> = layout.ordered.iter().map(|p| p.folder.clone()).collect();
    let after_violations: BTreeSet<(String, String)> = dependency_violations(layout, &order)
        .into_iter()
        .map(|v| (v.plugin, v.requires))
        .collect();
    let fixed = dependency_violations(layout, &before)
        .into_iter()
        .filter(|v| !after_violations.contains(&(v.plugin.clone(), v.requires.clone())))
        .collect();
    Suggestion { order, fixed }
}

// ---------------------------------------------------------------------------
// The effect on overlaps
// ---------------------------------------------------------------------------

/// A plugin as the overlap comparison needs it.
pub struct OverlapPlugin<'a> {
    /// Its folder name now.
    pub folder: &'a str,
    pub identity: &'a str,
    pub index: &'a PluginIndex,
}

/// Something two or more plugins both touch, where the plugin that wins would change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinnerChange {
    pub item: OverlapItem,
    /// Identity of the plugin whose values are used now; `None` if nobody overrides anybody.
    pub was: Option<String>,
    /// Identity of the winner after the change; `None` if the clash would go away.
    pub now: Option<String>,
    /// Who loses after the change.
    pub overridden: Vec<String>,
    /// Plain-language reason the new winner wins.
    pub reason: String,
}

/// The overlaps (see [`crate::overlap`]) whose winner differs between the current folder names
/// and the names after `plan`. `plugins` should be the enabled plugins, all of them, whether or
/// not the plan renames them.
pub fn winner_changes(
    backend: &dyn LoadOrderBackend,
    plugins: &[OverlapPlugin],
    plan: &ChangePlan,
) -> Vec<WinnerChange> {
    let after_names: Vec<String> = plugins
        .iter()
        .map(|p| backend.sort_key_after(plan, p.folder))
        .collect();
    let before: Vec<PluginEntry> = plugins
        .iter()
        .map(|p| PluginEntry {
            folder: p.folder,
            index: p.index,
        })
        .collect();
    let after: Vec<PluginEntry> = plugins
        .iter()
        .zip(&after_names)
        .map(|(p, name)| PluginEntry {
            folder: name,
            index: p.index,
        })
        .collect();
    let identity_before: HashMap<&str, &str> =
        plugins.iter().map(|p| (p.folder, p.identity)).collect();
    let identity_after: HashMap<&str, &str> = plugins
        .iter()
        .zip(&after_names)
        .map(|(p, name)| (name.as_str(), p.identity))
        .collect();

    let overlaps_before = overlap::find_overlaps(&before);
    let overlaps_after = overlap::find_overlaps(&after);
    let winner_before: BTreeMap<&OverlapItem, &str> = overlaps_before
        .iter()
        .filter_map(|o| {
            identity_before
                .get(o.winner.as_str())
                .map(|id| (&o.item, *id))
        })
        .collect();
    let winner_after: BTreeMap<&OverlapItem, &str> = overlaps_after
        .iter()
        .filter_map(|o| {
            identity_after
                .get(o.winner.as_str())
                .map(|id| (&o.item, *id))
        })
        .collect();

    let mut items: Vec<&OverlapItem> = winner_before
        .keys()
        .chain(winner_after.keys())
        .copied()
        .collect();
    items.sort();
    items.dedup();
    let mut changes = Vec::new();
    for item in items {
        let was = winner_before.get(item).copied();
        let now = winner_after.get(item).copied();
        if was == now {
            continue;
        }
        let (overridden, reason) = match overlaps_after.iter().find(|o| &o.item == item) {
            Some(o) => {
                let losers: Vec<String> = o
                    .overridden
                    .iter()
                    .filter_map(|f| identity_after.get(f.as_str()).map(|s| s.to_string()))
                    .collect();
                let reason = format!(
                    "{} loads after {} (folder {:?} sorts after {}).",
                    now.unwrap_or("?"),
                    losers.join(", "),
                    o.winner,
                    o.overridden
                        .iter()
                        .map(|f| format!("{f:?}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                (losers, reason)
            }
            None => (
                Vec::new(),
                "After the change no plugin sets this over another one.".to_string(),
            ),
        };
        changes.push(WinnerChange {
            item: item.clone(),
            was: was.map(str::to_string),
            now: now.map(str::to_string),
            overridden,
            reason,
        });
    }
    changes
}

// ---------------------------------------------------------------------------
// The folder-prefix backend
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
struct FolderPrefixBackend;

impl LoadOrderBackend for FolderPrefixBackend {
    fn name(&self) -> &'static str {
        "folder prefix"
    }

    fn layout(&self, installed: &[InstalledPlugin]) -> Layout {
        let mut layout = Layout::default();
        for plugin in installed {
            let reasons = exclusion_reasons(plugin);
            let renamed = plugin
                .record
                .as_ref()
                .is_some_and(|r| r.original_folder.is_some());
            if renamed {
                layout.restorable.push(plugin.folder.clone());
            }
            if reasons.is_empty() {
                let original = original_of(plugin);
                layout.ordered.push(OrderedPlugin {
                    number: parse_number(&plugin.folder, &original),
                    folder: plugin.folder.clone(),
                    identity: plugin.identity.clone(),
                    original_folder: original,
                    requires: plugin.meta.dependencies.requires.iter().cloned().collect(),
                });
            } else {
                layout.excluded.push(Excluded {
                    folder: plugin.folder.clone(),
                    identity: plugin.identity.clone(),
                    reasons,
                    still_renamed: renamed,
                });
            }
        }
        layout
            .ordered
            .sort_by(|a, b| game_order(&a.folder, &b.folder));
        layout
            .excluded
            .sort_by(|a, b| game_order(&a.folder, &b.folder));
        layout.restorable.sort();
        layout
    }

    fn plan_apply(
        &self,
        paths: &Paths,
        installed: &[InstalledPlugin],
        wanted: &[String],
    ) -> Result<ChangePlan, LoadOrderError> {
        let layout = self.layout(installed);
        let by_folder: HashMap<&str, &OrderedPlugin> = layout
            .ordered
            .iter()
            .map(|p| (p.folder.as_str(), p))
            .collect();
        let wanted_set: HashSet<&str> = wanted.iter().map(String::as_str).collect();
        if wanted.len() != layout.ordered.len()
            || wanted_set.len() != wanted.len()
            || !wanted_set.iter().all(|f| by_folder.contains_key(f))
        {
            return Err(LoadOrderError::OrderMismatch);
        }

        let current: Vec<Option<u32>> = wanted
            .iter()
            .map(|f| by_folder[f.as_str()].number)
            .collect();
        let numbers = assign_numbers(&current)?;

        let mut renames = Vec::new();
        let mut final_names = Vec::new();
        for (folder, number) in wanted.iter().zip(numbers) {
            let plugin = by_folder[folder.as_str()];
            let name = numbered_name(number, &plugin.original_folder);
            final_names.push(name.clone());
            if name != plugin.folder {
                renames.push(Rename {
                    from: plugin.folder.clone(),
                    to: name,
                    identity: plugin.identity.clone(),
                    from_original: (plugin.original_folder != plugin.folder)
                        .then(|| plugin.original_folder.clone()),
                    to_original: Some(plugin.original_folder.clone()),
                });
            }
        }
        check_targets(paths, &renames, true)?;

        let last = final_names.iter().max_by(|a, b| game_order(a, b));
        let in_order: HashSet<&str> = wanted.iter().map(String::as_str).collect();
        let mut loads_among: Vec<String> = installed
            .iter()
            .filter(|p| !in_order.contains(p.folder.as_str()))
            .filter(|p| last.is_some_and(|l| game_order(&p.folder, l) == Ordering::Less))
            .map(|p| p.folder.clone())
            .collect();
        loads_among.sort_by(|a, b| game_order(a, b));
        Ok(ChangePlan {
            kind: ChangeKind::Apply,
            renames,
            loads_among,
            identity_changes: Vec::new(),
        })
    }

    fn plan_restore(
        &self,
        paths: &Paths,
        installed: &[InstalledPlugin],
    ) -> Result<ChangePlan, LoadOrderError> {
        let mut renames: Vec<Rename> = installed
            .iter()
            .filter_map(|p| {
                let original = p.record.as_ref()?.original_folder.as_ref()?;
                (original != &p.folder).then(|| Rename {
                    from: p.folder.clone(),
                    to: original.clone(),
                    identity: p.identity.clone(),
                    from_original: Some(original.clone()),
                    to_original: None,
                })
            })
            .collect();
        renames.sort_by(|a, b| game_order(&a.from, &b.from));
        check_targets(paths, &renames, false)?;
        // A plugin with no `name` in its plugin.txt is known by its folder name, so putting
        // the original name back changes who it is (and which `plugins.txt` entry is its own).
        let mut identity_changes: Vec<(String, String)> = installed
            .iter()
            .filter(|p| is_blank(&p.meta.name))
            .filter_map(|p| {
                let rename = renames.iter().find(|r| r.from == p.folder)?;
                Some((p.folder.clone(), rename.to.clone()))
            })
            .collect();
        identity_changes.sort();
        Ok(ChangePlan {
            kind: ChangeKind::Restore,
            renames,
            loads_among: Vec::new(),
            identity_changes,
        })
    }

    fn commit(
        &self,
        paths: &Paths,
        plan: &ChangePlan,
        game: GameProcess,
    ) -> Result<CommitOutcome, LoadOrderError> {
        commit_with(paths, plan, game, &|from, to| fs::rename(from, to))
    }

    fn recover(&self, paths: &Paths, game: GameProcess) -> Result<Recovery, LoadOrderError> {
        recover_with(paths, game, &|from, to| fs::rename(from, to))
    }
}

type RenameFn<'a> = &'a dyn Fn(&Path, &Path) -> io::Result<()>;

/// A single folder name: the installer's own rules, so no separators, no `..`, nothing odd.
fn is_safe_name(name: &str) -> bool {
    !name.is_empty() && crate::install::sanitize_folder_name(name) == name
}

fn lower(name: &str) -> String {
    name.to_lowercase()
}

/// Refuses a plan whose new names are too long, collide with anything already in `plugins/`
/// (compared without regard to case, since Windows and macOS don't distinguish it), collide
/// with each other, or (when `check_paths`) would push a file path over the platform limit.
fn check_targets(
    paths: &Paths,
    renames: &[Rename],
    check_paths: bool,
) -> Result<(), LoadOrderError> {
    if renames.is_empty() {
        return Ok(());
    }
    let plugins = paths.plugins_dir();
    let mut existing: HashMap<String, String> = HashMap::new();
    match fs::read_dir(&plugins) {
        Ok(entries) => {
            for entry in entries {
                let name = entry?.file_name().to_string_lossy().into_owned();
                existing.insert(lower(&name), name);
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut targets: HashMap<String, &str> = HashMap::new();
    for rename in renames {
        if rename.to.len() > MAX_FOLDER_NAME_BYTES {
            return Err(LoadOrderError::NameTooLong {
                name: rename.to.clone(),
                length: rename.to.len(),
            });
        }
        // Names come from folders and from records.json; a name with a path separator or `..`
        // in it must never be joined onto plugins/.
        if !is_safe_name(&rename.from) || !is_safe_name(&rename.to) {
            return Err(LoadOrderError::Records(format!(
                "{:?} or {:?} isn't a plain folder name",
                rename.from, rename.to
            )));
        }
        if let Some(existing) = existing.get(&lower(&rename.to)) {
            return Err(LoadOrderError::Collision {
                name: rename.to.clone(),
                existing: existing.clone(),
            });
        }
        // Two new names that differ only by case are accepted here: the originals could only
        // have coexisted on a filesystem that tells them apart, and there they can coexist
        // again. (On one that doesn't, the second rename would fail at commit and everything
        // would be put back; that cannot arise from a restore of names that coexisted.)
        if let Some(other) = targets.insert(rename.to.clone(), &rename.from) {
            return Err(LoadOrderError::Collision {
                name: rename.to.clone(),
                existing: format!("the new name of {other:?}"),
            });
        }
        if check_paths && let Some(limit) = paths.max_path_chars {
            let base = plugins.to_string_lossy().encode_utf16().count();
            let deepest = longest_relative_path(&plugins.join(&rename.from))?;
            let length = base + 1 + rename.to.encode_utf16().count() + deepest;
            if length > limit {
                return Err(LoadOrderError::PathTooLong {
                    folder: rename.from.clone(),
                    length,
                    limit,
                });
            }
        }
    }
    Ok(())
}

/// The longest path below `dir`, in UTF-16 units counting the separator before each part (0 if
/// the folder is empty). Symlinks are not followed.
fn longest_relative_path(dir: &Path) -> io::Result<usize> {
    let mut longest = 0;
    let mut pending = vec![(dir.to_path_buf(), 0usize)];
    while let Some((path, depth_len)) = pending.pop() {
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            let len = depth_len + 1 + entry.file_name().to_string_lossy().encode_utf16().count();
            longest = longest.max(len);
            if entry.file_type()?.is_dir() {
                pending.push((entry.path(), len));
            }
        }
    }
    Ok(longest)
}

#[derive(Debug, Serialize, Deserialize)]
struct Journal {
    format: u32,
    renames: Vec<Rename>,
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Copies `plugins.txt` to its backup file (atomically), if there is one.
fn backup_plugin_states(config_dir: &Path) -> Result<Option<PathBuf>, LoadOrderError> {
    let source = config_dir.join(game_state::PLUGINS_FILE);
    let backup = config_dir.join(BACKUP_FILE);
    match fs::read(&source) {
        Ok(bytes) => {
            files::write_atomic(&backup, &bytes)?;
            Ok(Some(backup))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn write_journal(path: &Path, renames: &[Rename]) -> Result<(), LoadOrderError> {
    let journal = Journal {
        format: 1,
        renames: renames.to_vec(),
    };
    let mut json =
        serde_json::to_vec_pretty(&journal).map_err(|e| LoadOrderError::Io(e.to_string()))?;
    json.push(b'\n');
    files::write_atomic(path, &json)?;
    Ok(())
}

/// Gets rid of a journal whose renames have all been undone. If it can't be deleted it is
/// emptied instead, so a later recovery can never replay renames that were taken back.
fn clear_journal(path: &Path) -> bool {
    if fs::remove_file(path).is_ok() || !exists(path) {
        return true;
    }
    write_journal(path, &[]).is_ok()
}

/// Clears the journal of a run that is over (finished or undone). A journal that lingers would
/// be replayed by the next recovery, so failing to clear it is an error, not something to ignore.
fn finish_journal(path: &Path) -> Result<(), LoadOrderError> {
    if clear_journal(path) {
        Ok(())
    } else {
        Err(LoadOrderError::Unrecoverable(
            "the change is complete, but its journal couldn't be removed".into(),
        ))
    }
}

/// One commit or recovery at a time, in the whole process: a recovery must never replay a
/// journal that a live commit is still working through. It is the same lock the "give it a
/// name" step (`plugin_naming`) takes, so a name is never written halfway through a rename.
fn run_lock() -> MutexGuard<'static, ()> {
    files::plugin_folders_lock()
}

/// True while either feature's journal says a run is unfinished: the load order's own, or an
/// interrupted "give it a name" step (`plugin_naming`), which recovery finishes first.
fn journal_pending(paths: &Paths) -> bool {
    exists(&paths.journal_path())
        || exists(
            &paths
                .records_path
                .with_file_name(crate::plugin_naming::JOURNAL_FILE),
        )
}

/// The record to keep when both names have one: the one that is this plugin's own (same
/// identity, and downloaded by the manager) wins over a record someone else made meanwhile.
fn better_record(
    rename: &Rename,
    a: Option<InstallRecord>,
    b: Option<InstallRecord>,
) -> Option<InstallRecord> {
    let score = |r: &InstallRecord| (r.identity == rename.identity, !r.sha256.is_empty());
    match (a, b) {
        (Some(a), Some(b)) => Some(if score(&b) > score(&a) { b } else { a }),
        (a, b) => a.or(b),
    }
}

/// Brings the install records in line with the folder names that are actually on disk: for
/// each rename, a record follows whichever of the two names exists. Safe to run twice.
fn sync_records(paths: &Paths, renames: &[Rename]) -> Result<(), LoadOrderError> {
    let plugins = paths.plugins_dir();
    let mut records =
        records::load(paths.records_path).map_err(|e| LoadOrderError::Records(e.to_string()))?;
    for rename in renames {
        let (name, original) = match (
            exists(&plugins.join(&rename.from)),
            exists(&plugins.join(&rename.to)),
        ) {
            (false, true) => (&rename.to, &rename.to_original),
            (true, false) => (&rename.from, &rename.from_original),
            _ => continue,
        };
        let at_from = records.remove(&rename.from);
        let at_to = records.remove(&rename.to);
        if let Some(mut record) = better_record(rename, at_from, at_to) {
            record.folder = name.clone();
            record.original_folder = original.clone();
            records.insert(name.clone(), record);
        }
    }
    records::save(paths.records_path, &records).map_err(|e| LoadOrderError::Records(e.to_string()))
}

/// Puts back every rename in `done` (newest first). Returns how many went back and whether
/// all of them did.
fn undo(plugins: &Path, done: &[Rename], rename: RenameFn) -> (usize, bool) {
    let (mut moved, mut all) = (0, true);
    for r in done.iter().rev() {
        let (from, to) = (plugins.join(&r.from), plugins.join(&r.to));
        if exists(&to) && !exists(&from) {
            match rename(&to, &from) {
                Ok(()) => moved += 1,
                Err(_) => all = false,
            }
        }
    }
    (moved, all)
}

/// For a renaming (not a restore): the folder still holds the plugin the plan was made for,
/// not something else that took its old name meanwhile. A restore is exempt because a plugin
/// that has lost its `name` is known by its folder name.
fn is_same_plugin(plugins: &Path, rename: &Rename) -> bool {
    if rename.to_original.is_none() {
        return true;
    }
    fs::read_to_string(plugins.join(&rename.from).join("plugin.txt"))
        .ok()
        .and_then(|text| PluginMeta::parse(&text).name)
        .as_deref()
        == Some(rename.identity.as_str())
}

fn commit_with(
    paths: &Paths,
    plan: &ChangePlan,
    game: GameProcess,
    rename: RenameFn,
) -> Result<CommitOutcome, LoadOrderError> {
    if game == GameProcess::Running {
        return Err(LoadOrderError::GameRunning);
    }
    if plan.renames.is_empty() {
        return Ok(CommitOutcome {
            renamed: 0,
            backup: None,
        });
    }
    let _lock = run_lock();
    let journal_path = paths.journal_path();
    if journal_pending(paths) {
        return Err(LoadOrderError::JournalPending);
    }
    let plugins = paths.plugins_dir();
    // Re-check the disk: the plan may be a little old.
    for r in &plan.renames {
        if !exists(&plugins.join(&r.from)) {
            return Err(LoadOrderError::Missing(r.from.clone()));
        }
        if !is_same_plugin(&plugins, r) {
            return Err(LoadOrderError::Changed(r.from.clone()));
        }
        if exists(&plugins.join(&r.to)) {
            return Err(LoadOrderError::Collision {
                name: r.to.clone(),
                existing: r.to.clone(),
            });
        }
    }
    let backup = backup_plugin_states(paths.config_dir)?;
    write_journal(&journal_path, &plan.renames)?;

    for (done, r) in plan.renames.iter().enumerate() {
        // Looked at again just before each rename: on Linux, renaming onto an empty folder
        // that appeared in the meantime would silently replace it.
        let outcome = if exists(&plugins.join(&r.to)) {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "a folder with that name appeared",
            ))
        } else {
            rename(&plugins.join(&r.from), &plugins.join(&r.to))
        };
        if let Err(e) = outcome {
            // Includes the one that just failed, in case it took effect despite the error.
            let (_, all_back) = undo(&plugins, &plan.renames[..=done], rename);
            let rolled_back = all_back && clear_journal(&journal_path);
            if !all_back {
                // Leave the records following whatever is on disk; the journal stays for recovery.
                let _ = sync_records(paths, &plan.renames);
            }
            return Err(LoadOrderError::RenameFailed {
                from: r.from.clone(),
                to: r.to.clone(),
                error: e.to_string(),
                rolled_back,
            });
        }
    }
    // The journal stays until the records agree with the disk, so a failure here is finished
    // by `recover` at the next start.
    sync_records(paths, &plan.renames)?;
    finish_journal(&journal_path)?;
    tracing::info!(
        renamed = plan.renames.len(),
        kind = ?plan.kind,
        "load order committed"
    );
    Ok(CommitOutcome {
        renamed: plan.renames.len(),
        backup,
    })
}

fn recover_with(
    paths: &Paths,
    game: GameProcess,
    rename: RenameFn,
) -> Result<Recovery, LoadOrderError> {
    let _lock = run_lock();
    let journal_path = paths.journal_path();
    let text = match fs::read_to_string(&journal_path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Recovery::Nothing),
        Err(e) => return Err(LoadOrderError::Journal(e.to_string())),
    };
    let journal: Journal =
        serde_json::from_str(&text).map_err(|e| LoadOrderError::Journal(e.to_string()))?;
    if journal.format > 1 {
        return Err(LoadOrderError::Journal(
            "it was written by a newer version of the manager".into(),
        ));
    }
    if game == GameProcess::Running {
        return Err(LoadOrderError::GameRunning);
    }
    let plugins = paths.plugins_dir();
    let renames = &journal.renames;
    if let Some(r) = renames
        .iter()
        .find(|r| !is_safe_name(&r.from) || !is_safe_name(&r.to))
    {
        return Err(LoadOrderError::Journal(format!(
            "it names {:?} and {:?}, which aren't plain folder names",
            r.from, r.to
        )));
    }

    // First try to carry the run through to the end.
    let mut problem: Option<String> = None;
    let mut moved = 0;
    for r in renames {
        let (from, to) = (plugins.join(&r.from), plugins.join(&r.to));
        match (exists(&from), exists(&to)) {
            (true, false) => {
                if !is_same_plugin(&plugins, r) {
                    problem = Some(format!(
                        "{:?} is no longer the plugin it was, so it won't be renamed",
                        r.from
                    ));
                    break;
                }
                if let Err(e) = rename(&from, &to) {
                    problem = Some(format!("couldn't rename {:?} to {:?}: {e}", r.from, r.to));
                    break;
                }
                moved += 1;
            }
            (true, true) => {
                problem = Some(format!(
                    "both {:?} and {:?} exist, so {:?} can't be renamed",
                    r.from, r.to, r.from
                ));
                break;
            }
            // Already renamed, or the plugin was removed meanwhile: nothing to do.
            (false, _) => {}
        }
    }
    match problem {
        None => {
            sync_records(paths, renames)?;
            finish_journal(&journal_path)?;
            tracing::info!(moved, "interrupted load-order run finished");
            Ok(Recovery::Finished { renames: moved })
        }
        Some(reason) => {
            // It can't be finished, so undo it.
            let (back, all_back) = undo(&plugins, renames, rename);
            if !all_back {
                // Keep the records in step with the disk, but leave the journal for another try.
                let _ = sync_records(paths, renames);
                return Err(LoadOrderError::Unrecoverable(reason));
            }
            sync_records(paths, renames)?;
            finish_journal(&journal_path)?;
            tracing::warn!(%reason, "interrupted load-order run rolled back");
            Ok(Recovery::RolledBack {
                renames: back,
                reason,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager;
    use crate::overlap::{Definition, Mode};
    use crate::records::{InstallRecord, InstallRecords};

    // --- helpers ---------------------------------------------------------

    /// A throwaway game config dir with a `plugins/` folder and a records file beside the
    /// journal location.
    struct World {
        /// Held only so the temp dir lives as long as the test.
        _dir: tempfile::TempDir,
        config: PathBuf,
        records_path: PathBuf,
    }

    impl World {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let config = dir.path().join("endless-sky");
            fs::create_dir_all(config.join("plugins")).unwrap();
            let records_path = dir.path().join("app-data/records.json");
            World {
                _dir: dir,
                config,
                records_path,
            }
        }

        fn paths(&self) -> Paths<'_> {
            Paths {
                config_dir: &self.config,
                records_path: &self.records_path,
                max_path_chars: None,
            }
        }

        fn plugins(&self) -> PathBuf {
            self.config.join("plugins")
        }

        /// A plugin installed by the manager: folder, record (with a download hash), and a
        /// `plugin.txt` naming it (unless `name` is `None`).
        fn add(&self, folder: &str, name: Option<&str>, requires: &[&str]) {
            let dir = self.plugins().join(folder);
            fs::create_dir_all(dir.join("data")).unwrap();
            fs::write(dir.join("data/x.txt"), "ship \"X\"\n").unwrap();
            let mut text = String::new();
            if let Some(name) = name {
                text += &format!("name \"{name}\"\n");
            }
            if !requires.is_empty() {
                text += "dependencies\n\trequires\n";
                for r in requires {
                    text += &format!("\t\t\"{r}\"\n");
                }
            }
            if !text.is_empty() {
                fs::write(dir.join("plugin.txt"), text).unwrap();
            }
            self.set_record(folder, name.unwrap_or(folder), "hash", None);
        }

        fn set_record(&self, folder: &str, identity: &str, sha256: &str, original: Option<&str>) {
            let mut records = self.records();
            records.insert(
                folder.to_string(),
                InstallRecord {
                    catalog_name: folder.to_string(),
                    folder: folder.to_string(),
                    identity: identity.to_string(),
                    version: "1".into(),
                    source_url: "https://example.com/x.zip".into(),
                    sha256: sha256.to_string(),
                    original_folder: original.map(str::to_string),
                    added_name: None,
                },
            );
            records::save(&self.records_path, &records).unwrap();
        }

        fn records(&self) -> InstallRecords {
            records::load(&self.records_path).unwrap()
        }

        fn installed(&self) -> Vec<InstalledPlugin> {
            manager::scan_installed(&self.plugins(), &self.records()).unwrap()
        }

        fn folders(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(self.plugins())
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort_by(|a, b| game_order(a, b));
            names
        }

        fn journal_exists(&self) -> bool {
            self.paths().journal_path().exists()
        }

        /// Whether anything called `outside` appeared next to `plugins/`.
        fn dir_outside_exists(&self) -> bool {
            self.config.join("outside").exists()
        }
    }

    fn backend() -> FolderPrefixBackend {
        FolderPrefixBackend
    }

    fn folders_of(layout: &Layout) -> Vec<String> {
        layout.ordered.iter().map(|p| p.folder.clone()).collect()
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // --- the game's order ------------------------------------------------

    #[test]
    fn the_game_sorts_by_byte_value_so_case_matters() {
        let mut names = strs(&[
            "zz-patch", "Zeta", "alpha", "010-Foo", "Alpha", "-dash", "_under",
        ]);
        names.sort_by(|a, b| game_order(a, b));
        // Same result as std::sort over std::filesystem::path with libstdc++ (checked with a
        // small C++ program when this was written).
        assert_eq!(
            names,
            [
                "-dash", "010-Foo", "Alpha", "Zeta", "_under", "alpha", "zz-patch"
            ]
        );
    }

    // --- numbering ---------------------------------------------------------

    #[test]
    fn fresh_numbers_are_steps_of_ten() {
        assert_eq!(assign_numbers(&[None, None, None]).unwrap(), [10, 20, 30]);
        assert_eq!(assign_numbers(&[]).unwrap(), Vec::<u32>::new());
    }

    #[test]
    fn many_plugins_fall_back_to_steps_of_one_and_the_limit_is_refused() {
        let numbers = assign_numbers(&vec![None; 100]).unwrap();
        assert_eq!(numbers[0], 1);
        assert_eq!(numbers[99], 100);
        let numbers = assign_numbers(&vec![None; 99]).unwrap();
        assert_eq!(numbers[98], 990);
        assert!(matches!(
            assign_numbers(&vec![None; 1000]),
            Err(LoadOrderError::TooMany(1000))
        ));
        assert_eq!(assign_numbers(&vec![None; 999]).unwrap()[998], 999);
    }

    #[test]
    fn moving_one_plugin_changes_only_that_plugins_number() {
        // Wanted order: the plugin that was last (40) now goes first.
        let n = assign_numbers(&[Some(40), Some(10), Some(20), Some(30)]).unwrap();
        assert_eq!(n, [5, 10, 20, 30]);
        // Moved into the middle.
        let n = assign_numbers(&[Some(10), Some(40), Some(20), Some(30)]).unwrap();
        assert_eq!(n[0], 10);
        assert_eq!(&n[2..], [20, 30]);
        assert!(10 < n[1] && n[1] < 20);
    }

    #[test]
    fn a_new_unnumbered_plugin_goes_after_the_last_number_in_steps_of_ten() {
        let n = assign_numbers(&[Some(10), Some(20), None, None]).unwrap();
        assert_eq!(n, [10, 20, 30, 40]);
    }

    #[test]
    fn gaps_are_spread_evenly_and_a_full_gap_renumbers_everyone() {
        let n = assign_numbers(&[Some(10), None, None, Some(40)]).unwrap();
        assert_eq!(n, [10, 20, 30, 40]);
        // No room between 10 and 11: start over.
        let n = assign_numbers(&[Some(10), None, Some(11)]).unwrap();
        assert_eq!(n, [10, 20, 30]);
        // Duplicate numbers can't both stay.
        let n = assign_numbers(&[Some(10), Some(10)]).unwrap();
        assert!(n[0] < n[1]);
    }

    #[test]
    fn numbers_always_increase_and_stay_in_range() {
        let cases: Vec<Vec<Option<u32>>> = vec![
            vec![Some(999), None, None],
            vec![None, None, Some(1)],
            vec![Some(5), Some(3), Some(8), None, Some(2)],
            vec![Some(500), Some(400), Some(300), Some(200), Some(100)],
            // Numbers a hand-edited record could hold: never trusted, never a panic.
            vec![None, Some(0)],
            vec![Some(0), None, Some(0)],
            vec![Some(1500), None],
            vec![Some(u32::MAX), Some(1000), None],
        ];
        for case in cases {
            let n = assign_numbers(&case).unwrap();
            assert_eq!(n.len(), case.len());
            assert!(n.windows(2).all(|w| w[0] < w[1]), "{case:?} -> {n:?}");
            assert!(
                n.iter().all(|&x| (1..=999).contains(&x)),
                "{case:?} -> {n:?}"
            );
        }
    }

    #[test]
    fn prefix_parsing_needs_the_exact_shape() {
        assert_eq!(
            parse_number("000-Foo", "Foo"),
            None,
            "0 is not a number we hand out"
        );
        assert_eq!(parse_number("010-Foo", "Foo"), Some(10));
        assert_eq!(parse_number("999-Foo", "Foo"), Some(999));
        assert_eq!(parse_number("010-010-Foo", "010-Foo"), Some(10));
        assert_eq!(parse_number("Foo", "Foo"), None);
        assert_eq!(parse_number("10-Foo", "Foo"), None);
        assert_eq!(parse_number("0100-Foo", "Foo"), None);
        assert_eq!(parse_number("010_Foo", "Foo"), None);
        assert_eq!(parse_number("abc-Foo", "Foo"), None);
        assert_eq!(numbered_name(5, "Foo"), "005-Foo");
    }

    // --- eligibility ---------------------------------------------------------

    #[test]
    fn named_plugins_the_manager_has_a_record_for_can_be_ordered() {
        let w = World::new();
        w.add("Zed", Some("Zed Plugin"), &[]);
        w.add("Anon", None, &[]); // no plugin.txt name: identity is the folder
        w.add("ByHand", Some("By Hand"), &[]);
        w.set_record("ByHand", "By Hand", "", None); // adopted: put in by hand, linked to the catalog
        // Not in the records at all.
        fs::create_dir_all(w.plugins().join("Loose/data")).unwrap();
        fs::write(w.plugins().join("Loose/plugin.txt"), "name \"Loose\"\n").unwrap();

        let layout = backend().layout(&w.installed());
        assert_eq!(folders_of(&layout), ["ByHand", "Zed"]);
        let reasons = |folder: &str| {
            layout
                .excluded
                .iter()
                .find(|e| e.folder == folder)
                .unwrap()
                .reasons
                .clone()
        };
        assert_eq!(reasons("Anon"), [ExcludeReason::NoPluginName]);
        assert_eq!(reasons("Loose"), [ExcludeReason::NotLinkedToCatalog]);
    }

    #[test]
    fn a_plugin_without_a_name_stays_out_even_when_it_was_put_in_by_hand() {
        let w = World::new();
        w.add("AnonLinked", None, &[]);
        w.set_record("AnonLinked", "AnonLinked", "", None); // adopted, but nameless
        fs::create_dir_all(w.plugins().join("AnonLoose/data")).unwrap(); // no record, no plugin.txt
        let layout = backend().layout(&w.installed());
        assert!(layout.ordered.is_empty());
        let reasons = |folder: &str| {
            layout
                .excluded
                .iter()
                .find(|e| e.folder == folder)
                .unwrap()
                .reasons
                .clone()
        };
        assert_eq!(reasons("AnonLinked"), [ExcludeReason::NoPluginName]);
        assert_eq!(
            reasons("AnonLoose"),
            [
                ExcludeReason::NotLinkedToCatalog,
                ExcludeReason::NoPluginName
            ]
        );
        // And a plan over them renames nothing.
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &[])
            .unwrap();
        assert!(plan.renames.is_empty());
    }

    #[test]
    fn a_plugin_put_in_by_hand_and_linked_to_the_catalog_is_renamed_and_restored_like_any_other() {
        let w = World::new();
        w.add("Foo-1.2", Some("Foo"), &[]);
        w.set_record("Foo-1.2", "Foo", "", None); // adopted: no download hash
        w.add("Bar", Some("Bar"), &[]);
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(&["Bar", "Foo-1.2"]))
            .unwrap();
        backend()
            .commit(&w.paths(), &plan, GameProcess::NotRunning)
            .unwrap();
        assert_eq!(w.folders(), ["010-Bar", "020-Foo-1.2"]);
        let records = w.records();
        assert_eq!(
            records["020-Foo-1.2"].original_folder.as_deref(),
            Some("Foo-1.2")
        );
        assert_eq!(records["020-Foo-1.2"].sha256, "", "still an adopted record");
        assert_eq!(records["020-Foo-1.2"].identity, "Foo");

        let restore = backend().plan_restore(&w.paths(), &w.installed()).unwrap();
        backend()
            .commit(&w.paths(), &restore, GameProcess::NotRunning)
            .unwrap();
        assert_eq!(w.folders(), ["Bar", "Foo-1.2"]);
        let records = w.records();
        assert!(
            !records.contains_key("020-Foo-1.2"),
            "the numbered key is gone"
        );
        let restored = &records["Foo-1.2"];
        assert_eq!(restored.original_folder, None, "nothing left to restore");
        assert_eq!(restored.folder, "Foo-1.2");
        assert_eq!(restored.sha256, "", "still an adopted record");
    }

    // Unix only: Windows can't have a folder with a colon in its name.
    #[cfg(unix)]
    #[test]
    fn hand_named_folders_the_manager_would_not_rename_are_left_out_with_a_reason() {
        let w = World::new();
        w.add("Fine", Some("Fine"), &[]);
        w.add("Odd:name", Some("Odd Name"), &[]); // a colon: fine on Linux, never on Windows
        let long = "y".repeat(252); // too long to take a "010-" in front
        w.add(&long, Some("Long"), &[]);
        let layout = backend().layout(&w.installed());
        assert_eq!(folders_of(&layout), ["Fine"]);
        for excluded in &layout.excluded {
            assert_eq!(
                excluded.reasons,
                [ExcludeReason::UnsupportedFolderName],
                "{}",
                excluded.folder
            );
        }
        assert_eq!(layout.excluded.len(), 2);
        // So a plan over the rest still works instead of failing as a whole.
        assert!(
            backend()
                .plan_apply(&w.paths(), &w.installed(), &strs(&["Fine"]))
                .is_ok()
        );
    }

    #[test]
    fn an_empty_name_counts_as_no_name() {
        let w = World::new();
        w.add("Odd", Some(" "), &[]);
        let layout = backend().layout(&w.installed());
        assert!(layout.ordered.is_empty());
        assert_eq!(layout.excluded[0].reasons, [ExcludeReason::NoPluginName]);
    }

    #[test]
    fn layout_lists_plugins_in_the_games_order() {
        let w = World::new();
        w.add("beta", Some("Beta"), &[]);
        w.add("Alpha", Some("Alpha P"), &[]);
        w.add("zz-patch", Some("Patch"), &[]);
        let layout = backend().layout(&w.installed());
        assert_eq!(folders_of(&layout), ["Alpha", "beta", "zz-patch"]);
    }

    // --- planning ------------------------------------------------------------

    #[test]
    fn applying_an_order_numbers_folders_in_steps_of_ten() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        w.add("Beta", Some("Beta"), &[]);
        w.add("Gamma", Some("Gamma"), &[]);
        let plan = backend()
            .plan_apply(
                &w.paths(),
                &w.installed(),
                &strs(&["Gamma", "Alpha", "Beta"]),
            )
            .unwrap();
        let renames: Vec<(&str, &str)> = plan
            .renames
            .iter()
            .map(|r| (r.from.as_str(), r.to.as_str()))
            .collect();
        assert_eq!(
            renames,
            [
                ("Gamma", "010-Gamma"),
                ("Alpha", "020-Alpha"),
                ("Beta", "030-Beta")
            ]
        );
        assert!(plan.renames.iter().all(|r| r.from_original.is_none()));
        assert_eq!(plan.renames[0].to_original.as_deref(), Some("Gamma"));
        // Planning changes nothing.
        assert_eq!(w.folders(), ["Alpha", "Beta", "Gamma"]);
    }

    #[test]
    fn a_plan_must_cover_exactly_the_orderable_plugins() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        w.add("Beta", Some("Beta"), &[]);
        let installed = w.installed();
        for wanted in [
            strs(&["Alpha"]),
            strs(&["Alpha", "Beta", "Gamma"]),
            strs(&["Alpha", "Alpha"]),
            strs(&["Alpha", "Nope"]),
        ] {
            assert!(
                matches!(
                    backend().plan_apply(&w.paths(), &installed, &wanted),
                    Err(LoadOrderError::OrderMismatch)
                ),
                "{wanted:?}"
            );
        }
    }

    #[test]
    fn a_name_collision_is_refused_whatever_the_case() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        // Something already called "010-alpha" (different case) sits in plugins/.
        fs::create_dir_all(w.plugins().join("010-alpha/data")).unwrap();
        let err = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(&["Alpha"]))
            .unwrap_err();
        assert!(
            matches!(&err, LoadOrderError::Collision { name, existing }
                if name == "010-Alpha" && existing == "010-alpha"),
            "{err:?}"
        );
        assert!(err.to_string().contains("010-alpha"));
    }

    #[test]
    fn a_folder_name_that_would_get_too_long_is_left_out_and_a_forced_one_is_still_refused() {
        let w = World::new();
        let long = "x".repeat(253);
        w.add(&long, Some("Long"), &[]);
        // Up front: it is simply not part of the order, with a reason.
        let layout = backend().layout(&w.installed());
        assert!(layout.ordered.is_empty());
        assert_eq!(
            layout.excluded[0].reasons,
            [ExcludeReason::UnsupportedFolderName]
        );
        // Defence in depth: a plan that somehow contained it is refused, not attempted.
        let rename = Rename {
            from: long.clone(),
            to: format!("010-{long}"),
            identity: "Long".into(),
            from_original: None,
            to_original: Some(long),
        };
        let err = check_targets(&w.paths(), &[rename], false).unwrap_err();
        assert!(
            matches!(err, LoadOrderError::NameTooLong { length: 257, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn a_path_over_the_windows_limit_is_refused() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        let deep = w.plugins().join("Alpha/images/ship");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("a-long-file-name.png"), b"x").unwrap();
        let base = w.plugins().to_string_lossy().encode_utf16().count();
        // "010-Alpha" is 9 chars; the longest file below is images/ship/a-long-file-name.png.
        let deepest = "/images/ship/a-long-file-name.png".len();
        let needed = base + 1 + 9 + deepest;

        let mut paths = w.paths();
        paths.max_path_chars = Some(needed);
        assert!(
            backend()
                .plan_apply(&paths, &w.installed(), &strs(&["Alpha"]))
                .is_ok()
        );
        paths.max_path_chars = Some(needed - 1);
        let err = backend()
            .plan_apply(&paths, &w.installed(), &strs(&["Alpha"]))
            .unwrap_err();
        assert!(
            matches!(err, LoadOrderError::PathTooLong { length, limit, .. }
                if length == needed && limit == needed - 1),
            "{err:?}"
        );
    }

    #[test]
    fn plugins_outside_the_order_are_reported_when_they_load_among_it() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        // Hand-installed, and its name sorts between the numbers we will hand out.
        fs::create_dir_all(w.plugins().join("015-Custom/data")).unwrap();
        // A plain name loads after the numbered ones, so it isn't mentioned.
        fs::create_dir_all(w.plugins().join("Plain/data")).unwrap();
        w.add("Beta", Some("Beta"), &[]);
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(&["Alpha", "Beta"]))
            .unwrap();
        assert_eq!(plan.loads_among, ["015-Custom"]);
    }

    #[test]
    fn unmanaged_and_unnamed_plugins_are_never_in_a_plan() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        w.add("Anon", None, &[]);
        fs::create_dir_all(w.plugins().join("Loose/data")).unwrap();
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(&["Alpha"]))
            .unwrap();
        assert_eq!(plan.renames.len(), 1);
        backend()
            .commit(&w.paths(), &plan, GameProcess::NotRunning)
            .unwrap();
        assert_eq!(w.folders(), ["010-Alpha", "Anon", "Loose"]);
        // Their contents were not touched either (nothing is ever written inside a plugin).
        assert_eq!(
            fs::read_to_string(w.plugins().join("Anon/data/x.txt")).unwrap(),
            "ship \"X\"\n"
        );
        assert!(w.records().contains_key("Anon"));
    }

    // --- committing ----------------------------------------------------------

    fn apply_all(w: &World, wanted: &[&str]) -> CommitOutcome {
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(wanted))
            .unwrap();
        backend()
            .commit(&w.paths(), &plan, GameProcess::NotRunning)
            .unwrap()
    }

    #[test]
    fn commit_renames_folders_and_keeps_the_records_in_step() {
        let w = World::new();
        w.add("Alpha", Some("Alpha P"), &[]);
        w.add("Beta", Some("Beta P"), &[]);
        fs::write(w.config.join("plugins.txt"), "state\n\t\"Alpha P\" 1\n").unwrap();

        let out = apply_all(&w, &["Beta", "Alpha"]);
        assert_eq!(out.renamed, 2);
        assert_eq!(out.backup, Some(w.config.join(BACKUP_FILE)));
        assert_eq!(
            fs::read_to_string(w.config.join(BACKUP_FILE)).unwrap(),
            "state\n\t\"Alpha P\" 1\n"
        );
        assert_eq!(w.folders(), ["010-Beta", "020-Alpha"]);
        assert!(!w.journal_exists());

        let records = w.records();
        assert!(!records.contains_key("Alpha"));
        let alpha = &records["020-Alpha"];
        assert_eq!(alpha.folder, "020-Alpha");
        assert_eq!(alpha.original_folder.as_deref(), Some("Alpha"));
        assert_eq!(alpha.identity, "Alpha P", "identity is untouched");

        // The scan finds the renamed folders through their records.
        let layout = backend().layout(&w.installed());
        assert_eq!(folders_of(&layout), ["010-Beta", "020-Alpha"]);
        assert_eq!(layout.ordered[0].number, Some(10));
        assert_eq!(layout.ordered[0].original_folder, "Beta");
        assert_eq!(layout.restorable, ["010-Beta", "020-Alpha"]);
        // Plugin state (by identity) was never touched.
        assert_eq!(
            fs::read_to_string(w.config.join("plugins.txt")).unwrap(),
            "state\n\t\"Alpha P\" 1\n"
        );
    }

    #[test]
    fn reordering_again_renames_only_what_moves() {
        let w = World::new();
        for n in ["Alpha", "Beta", "Gamma", "Delta"] {
            w.add(n, Some(n), &[]);
        }
        apply_all(&w, &["Alpha", "Beta", "Gamma", "Delta"]);
        assert_eq!(
            w.folders(),
            ["010-Alpha", "020-Beta", "030-Gamma", "040-Delta"]
        );

        // Move Delta to the front: only Delta is renamed.
        let plan = backend()
            .plan_apply(
                &w.paths(),
                &w.installed(),
                &strs(&["040-Delta", "010-Alpha", "020-Beta", "030-Gamma"]),
            )
            .unwrap();
        assert_eq!(plan.renames.len(), 1);
        assert_eq!(plan.renames[0].from, "040-Delta");
        assert_eq!(plan.renames[0].to, "005-Delta");
        assert_eq!(plan.renames[0].from_original.as_deref(), Some("Delta"));
        backend()
            .commit(&w.paths(), &plan, GameProcess::NotRunning)
            .unwrap();
        assert_eq!(
            w.folders(),
            ["005-Delta", "010-Alpha", "020-Beta", "030-Gamma"]
        );
        assert_eq!(
            w.records()["005-Delta"].original_folder.as_deref(),
            Some("Delta")
        );
    }

    #[test]
    fn nothing_to_change_is_a_no_op() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        apply_all(&w, &["Alpha"]);
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(&["010-Alpha"]))
            .unwrap();
        assert!(plan.renames.is_empty());
        let out = backend()
            .commit(&w.paths(), &plan, GameProcess::NotRunning)
            .unwrap();
        assert_eq!(out.renamed, 0);
    }

    #[test]
    fn nothing_is_renamed_while_the_game_runs() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(&["Alpha"]))
            .unwrap();
        let err = backend()
            .commit(&w.paths(), &plan, GameProcess::Running)
            .unwrap_err();
        assert!(matches!(err, LoadOrderError::GameRunning));
        assert_eq!(w.folders(), ["Alpha"]);
        assert!(!w.journal_exists());
        // If the process list can't be read the rename proceeds, same as plugins.txt writes.
        backend()
            .commit(&w.paths(), &plan, GameProcess::Unknown)
            .unwrap();
        assert_eq!(w.folders(), ["010-Alpha"]);
    }

    #[test]
    fn a_stale_plan_is_refused_before_anything_moves() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        w.add("Beta", Some("Beta"), &[]);
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(&["Alpha", "Beta"]))
            .unwrap();
        // Someone creates the target of the second rename after planning.
        fs::create_dir_all(w.plugins().join("020-Beta")).unwrap();
        let err = backend()
            .commit(&w.paths(), &plan, GameProcess::NotRunning)
            .unwrap_err();
        assert!(matches!(err, LoadOrderError::Collision { .. }), "{err:?}");
        assert_eq!(w.folders(), ["020-Beta", "Alpha", "Beta"]);
        assert!(!w.journal_exists());

        fs::remove_dir_all(w.plugins().join("020-Beta")).unwrap();
        fs::remove_dir_all(w.plugins().join("Beta")).unwrap();
        let err = backend()
            .commit(&w.paths(), &plan, GameProcess::NotRunning)
            .unwrap_err();
        assert!(matches!(err, LoadOrderError::Missing(f) if f == "Beta"));
    }

    #[test]
    fn a_failed_rename_puts_everything_back() {
        let w = World::new();
        for n in ["Alpha", "Beta", "Gamma"] {
            w.add(n, Some(n), &[]);
        }
        let plan = backend()
            .plan_apply(
                &w.paths(),
                &w.installed(),
                &strs(&["Alpha", "Beta", "Gamma"]),
            )
            .unwrap();
        let calls = std::cell::Cell::new(0);
        let flaky = |from: &Path, to: &Path| {
            calls.set(calls.get() + 1);
            if calls.get() == 3 {
                Err(io::Error::other("disk on fire"))
            } else {
                fs::rename(from, to)
            }
        };
        let err = commit_with(&w.paths(), &plan, GameProcess::NotRunning, &flaky).unwrap_err();
        assert!(
            matches!(&err, LoadOrderError::RenameFailed { rolled_back: true, from, .. } if from == "Gamma"),
            "{err:?}"
        );
        assert!(err.to_string().contains("put back"));
        assert_eq!(w.folders(), ["Alpha", "Beta", "Gamma"]);
        assert!(!w.journal_exists());
        assert!(w.records().contains_key("Alpha"), "records untouched");
    }

    #[test]
    fn a_failed_rename_that_cannot_be_undone_keeps_the_journal() {
        let w = World::new();
        for n in ["Alpha", "Beta"] {
            w.add(n, Some(n), &[]);
        }
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(&["Alpha", "Beta"]))
            .unwrap();
        let calls = std::cell::Cell::new(0);
        let broken = |from: &Path, to: &Path| {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                fs::rename(from, to)
            } else {
                Err(io::Error::other("nope"))
            }
        };
        let err = commit_with(&w.paths(), &plan, GameProcess::NotRunning, &broken).unwrap_err();
        assert!(matches!(
            err,
            LoadOrderError::RenameFailed {
                rolled_back: false,
                ..
            }
        ));
        assert!(w.journal_exists(), "the journal is kept for the next start");
        assert_eq!(w.folders(), ["010-Alpha", "Beta"]);

        // And the next start straightens it out.
        let out = backend()
            .recover(&w.paths(), GameProcess::NotRunning)
            .unwrap();
        assert_eq!(out, Recovery::Finished { renames: 1 });
        assert_eq!(w.folders(), ["010-Alpha", "020-Beta"]);
        assert!(!w.journal_exists());
    }

    #[test]
    fn the_naming_step_and_the_manager_agree_with_this_module() {
        // They spell these out so they don't depend on this file.
        assert_eq!(crate::manager::UNFINISHED_CHANGE_JOURNALS[0], JOURNAL_FILE);
        assert_eq!(crate::plugin_naming::LOAD_ORDER_JOURNAL_FILE, JOURNAL_FILE);
        // "A folder name the load order numbers" means the same in both places.
        let long_ok = "x".repeat(251);
        let long_bad = "x".repeat(252);
        for folder in [
            "Plain",
            "Mega Freight",
            "Odd.",
            "trailing ",
            "con",
            "a/b",
            "a:b",
            long_ok.as_str(),
            long_bad.as_str(),
        ] {
            let plugin = InstalledPlugin {
                folder: folder.to_string(),
                identity: "Named".into(),
                meta: PluginMeta {
                    name: Some("Named".into()),
                    ..Default::default()
                },
                record: Some(InstallRecord {
                    catalog_name: "C".into(),
                    folder: folder.to_string(),
                    identity: "Named".into(),
                    version: "1".into(),
                    source_url: String::new(),
                    sha256: "h".into(),
                    original_folder: None,
                    added_name: None,
                }),
            };
            assert_eq!(
                exclusion_reasons(&plugin).contains(&ExcludeReason::UnsupportedFolderName),
                !crate::plugin_naming::folder_can_take_a_number(folder),
                "{folder:?}"
            );
        }
    }

    #[test]
    fn a_pending_journal_blocks_new_changes() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(&["Alpha"]))
            .unwrap();
        write_journal(&w.paths().journal_path(), &plan.renames).unwrap();
        let err = backend()
            .commit(&w.paths(), &plan, GameProcess::NotRunning)
            .unwrap_err();
        assert!(matches!(err, LoadOrderError::JournalPending));

        // So does an unfinished "give it a name" step: it must be finished before any folder
        // moves, or it could no longer find the plugin it was naming.
        fs::remove_file(w.paths().journal_path()).unwrap();
        let naming = w
            .records_path
            .with_file_name(crate::plugin_naming::JOURNAL_FILE);
        fs::write(&naming, "{}").unwrap();
        let err = backend()
            .commit(&w.paths(), &plan, GameProcess::NotRunning)
            .unwrap_err();
        assert!(matches!(err, LoadOrderError::JournalPending));
        assert_eq!(w.folders(), ["Alpha"]);
        // And the naming step looks for this module's journal under the same name.
        assert_eq!(crate::plugin_naming::LOAD_ORDER_JOURNAL_FILE, JOURNAL_FILE);
    }

    // --- recovery ------------------------------------------------------------

    /// A crash simulated by hand: the journal is written, then only the first `k` renames
    /// happen (and the records are not updated), exactly as if the process had been killed.
    fn crash_after(w: &World, wanted: &[&str], k: usize) -> ChangePlan {
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(wanted))
            .unwrap();
        write_journal(&w.paths().journal_path(), &plan.renames).unwrap();
        for r in &plan.renames[..k] {
            fs::rename(w.plugins().join(&r.from), w.plugins().join(&r.to)).unwrap();
        }
        plan
    }

    #[test]
    fn recovery_finishes_a_run_interrupted_before_any_rename() {
        let w = World::new();
        for n in ["Alpha", "Beta", "Gamma"] {
            w.add(n, Some(n), &[]);
        }
        crash_after(&w, &["Gamma", "Beta", "Alpha"], 0);
        assert_eq!(w.folders(), ["Alpha", "Beta", "Gamma"]);
        let out = backend()
            .recover(&w.paths(), GameProcess::NotRunning)
            .unwrap();
        assert_eq!(out, Recovery::Finished { renames: 3 });
        assert_eq!(w.folders(), ["010-Gamma", "020-Beta", "030-Alpha"]);
        assert!(!w.journal_exists());
        assert_eq!(
            w.records()["030-Alpha"].original_folder.as_deref(),
            Some("Alpha")
        );
        assert_eq!(w.records().len(), 3);
    }

    #[test]
    fn recovery_finishes_a_run_interrupted_halfway() {
        let w = World::new();
        for n in ["Alpha", "Beta", "Gamma"] {
            w.add(n, Some(n), &[]);
        }
        crash_after(&w, &["Alpha", "Beta", "Gamma"], 2);
        assert_eq!(w.folders(), ["010-Alpha", "020-Beta", "Gamma"]);
        let out = backend()
            .recover(&w.paths(), GameProcess::NotRunning)
            .unwrap();
        assert_eq!(out, Recovery::Finished { renames: 1 });
        assert_eq!(w.folders(), ["010-Alpha", "020-Beta", "030-Gamma"]);
        let records = w.records();
        assert_eq!(records.len(), 3);
        for (folder, original) in [
            ("010-Alpha", "Alpha"),
            ("020-Beta", "Beta"),
            ("030-Gamma", "Gamma"),
        ] {
            assert_eq!(records[folder].folder, folder);
            assert_eq!(records[folder].original_folder.as_deref(), Some(original));
        }
    }

    #[test]
    fn recovery_finishes_a_run_whose_records_were_not_saved() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        crash_after(&w, &["Alpha"], 1);
        // All renames done, records still under the old key, journal still there.
        assert!(w.records().contains_key("Alpha"));
        let out = backend()
            .recover(&w.paths(), GameProcess::NotRunning)
            .unwrap();
        assert_eq!(out, Recovery::Finished { renames: 0 });
        assert!(w.records().contains_key("010-Alpha"));
        assert!(!w.records().contains_key("Alpha"));
    }

    #[test]
    fn recovery_is_a_no_op_once_the_run_is_complete() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        let plan = crash_after(&w, &["Alpha"], 1);
        sync_records(&w.paths(), &plan.renames).unwrap();
        // Crash between saving the records and removing the journal.
        let out = backend()
            .recover(&w.paths(), GameProcess::NotRunning)
            .unwrap();
        assert_eq!(out, Recovery::Finished { renames: 0 });
        assert_eq!(w.records().len(), 1);
        assert_eq!(w.folders(), ["010-Alpha"]);
        assert_eq!(
            backend()
                .recover(&w.paths(), GameProcess::NotRunning)
                .unwrap(),
            Recovery::Nothing
        );
    }

    #[test]
    fn recovery_rolls_back_when_it_cannot_finish() {
        let w = World::new();
        for n in ["Alpha", "Beta"] {
            w.add(n, Some(n), &[]);
        }
        crash_after(&w, &["Alpha", "Beta"], 1);
        // While the manager was down, somebody put a folder where Beta was going.
        fs::create_dir_all(w.plugins().join("020-Beta/data")).unwrap();
        let out = backend()
            .recover(&w.paths(), GameProcess::NotRunning)
            .unwrap();
        assert!(
            matches!(out, Recovery::RolledBack { renames: 1, .. }),
            "{out:?}"
        );
        assert_eq!(w.folders(), ["020-Beta", "Alpha", "Beta"]);
        assert!(w.records().contains_key("Alpha"));
        assert!(w.records()["Alpha"].original_folder.is_none());
        assert!(!w.journal_exists());
    }

    #[test]
    fn recovery_waits_while_the_game_runs_and_reports_a_corrupt_journal() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        crash_after(&w, &["Alpha"], 0);
        assert!(matches!(
            backend().recover(&w.paths(), GameProcess::Running),
            Err(LoadOrderError::GameRunning)
        ));
        assert!(w.journal_exists());

        fs::write(w.paths().journal_path(), "{not json").unwrap();
        assert!(matches!(
            backend().recover(&w.paths(), GameProcess::NotRunning),
            Err(LoadOrderError::Journal(_))
        ));
        assert!(w.journal_exists(), "an unreadable journal is never deleted");
        assert_eq!(w.folders(), ["Alpha"]);
    }

    #[test]
    fn a_plugin_removed_during_the_interruption_is_skipped() {
        let w = World::new();
        for n in ["Alpha", "Beta"] {
            w.add(n, Some(n), &[]);
        }
        crash_after(&w, &["Alpha", "Beta"], 0);
        fs::remove_dir_all(w.plugins().join("Beta")).unwrap();
        let out = backend()
            .recover(&w.paths(), GameProcess::NotRunning)
            .unwrap();
        assert_eq!(out, Recovery::Finished { renames: 1 });
        assert_eq!(w.folders(), ["010-Alpha"]);
    }

    // --- restoring -----------------------------------------------------------

    #[test]
    fn restore_puts_every_original_name_back() {
        let w = World::new();
        for n in ["Alpha", "Beta", "Gamma"] {
            w.add(n, Some(n), &[]);
        }
        apply_all(&w, &["Gamma", "Alpha", "Beta"]);
        assert_eq!(w.folders(), ["010-Gamma", "020-Alpha", "030-Beta"]);

        let plan = backend().plan_restore(&w.paths(), &w.installed()).unwrap();
        assert_eq!(plan.kind, ChangeKind::Restore);
        assert_eq!(plan.renames.len(), 3);
        backend()
            .commit(&w.paths(), &plan, GameProcess::NotRunning)
            .unwrap();
        assert_eq!(w.folders(), ["Alpha", "Beta", "Gamma"]);
        let records = w.records();
        assert_eq!(records.len(), 3);
        assert!(records.values().all(|r| r.original_folder.is_none()));
        assert!(records.values().all(|r| r.folder == r.identity));
        assert!(backend().layout(&w.installed()).restorable.is_empty());
        // Nothing left to restore.
        assert!(
            backend()
                .plan_restore(&w.paths(), &w.installed())
                .unwrap()
                .renames
                .is_empty()
        );
    }

    #[test]
    fn restore_refuses_when_an_original_name_is_taken() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        apply_all(&w, &["Alpha"]);
        fs::create_dir_all(w.plugins().join("alpha/data")).unwrap();
        let err = backend()
            .plan_restore(&w.paths(), &w.installed())
            .unwrap_err();
        assert!(matches!(err, LoadOrderError::Collision { .. }), "{err:?}");
        assert_eq!(w.folders(), ["010-Alpha", "alpha"]);
    }

    #[test]
    fn restore_works_for_a_plugin_that_lost_its_name_and_is_no_longer_orderable() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        apply_all(&w, &["Alpha"]);
        // An update replaced plugin.txt with one that has no name.
        fs::write(w.plugins().join("010-Alpha/plugin.txt"), "version 2\n").unwrap();
        let installed = w.installed();
        let layout = backend().layout(&installed);
        assert!(layout.ordered.is_empty());
        assert!(layout.excluded[0].still_renamed);
        let plan = backend().plan_restore(&w.paths(), &installed).unwrap();
        assert_eq!(plan.renames.len(), 1);
        backend()
            .commit(&w.paths(), &plan, GameProcess::NotRunning)
            .unwrap();
        assert_eq!(w.folders(), ["Alpha"]);
    }

    // --- dependencies ----------------------------------------------------------

    fn layout_of(w: &World) -> Layout {
        backend().layout(&w.installed())
    }

    #[test]
    fn the_suggestion_puts_requirements_first_and_changes_little_else() {
        let w = World::new();
        // Game order: A, B, C, D. A requires D; C requires B (already earlier).
        w.add("A", Some("A"), &["D"]);
        w.add("B", Some("B"), &[]);
        w.add("C", Some("C"), &["B"]);
        w.add("D", Some("D"), &[]);
        let layout = layout_of(&w);
        let suggestion = suggest_order(&layout);
        assert_eq!(suggestion.order, ["D", "A", "B", "C"]);
        assert_eq!(
            suggestion.fixed,
            [Violation {
                plugin: "A".into(),
                requires: "D".into()
            }]
        );
        assert!(dependency_violations(&layout, &suggestion.order).is_empty());
    }

    #[test]
    fn requirements_that_cannot_be_placed_are_ignored_and_cycles_end() {
        let w = World::new();
        w.add("A", Some("A"), &["Not Installed", "Hand Made"]);
        w.add("B", Some("B"), &["C"]);
        w.add("C", Some("C"), &["B"]); // a cycle
        w.add("Hand", Some("Hand Made"), &[]);
        // Not in the records at all, so it can't be ordered and its requirement is ignored.
        fs::remove_file(w.records_path.clone()).unwrap();
        for folder in ["A", "B", "C"] {
            w.set_record(folder, folder, "hash", None);
        }
        let layout = layout_of(&w);
        let suggestion = suggest_order(&layout);
        let mut sorted = suggestion.order.clone();
        sorted.sort();
        assert_eq!(sorted, ["A", "B", "C"], "every plugin appears exactly once");
        assert_eq!(suggestion.order[0], "A", "A has nothing it can wait for");
    }

    #[test]
    fn violations_name_the_plugin_and_the_requirement_that_loads_later() {
        let w = World::new();
        w.add("A", Some("A"), &["B"]);
        w.add("B", Some("B"), &[]);
        let layout = layout_of(&w);
        assert!(dependency_violations(&layout, &strs(&["B", "A"])).is_empty());
        assert_eq!(
            dependency_violations(&layout, &strs(&["A", "B"])),
            [Violation {
                plugin: "A".into(),
                requires: "B".into()
            }]
        );
    }

    // --- overlaps --------------------------------------------------------------

    fn index_setting(kind: &str, name: &str, field: &str) -> PluginIndex {
        PluginIndex {
            format: overlap::INDEX_FORMAT,
            fingerprint: String::new(),
            definitions: [Definition {
                kind: kind.into(),
                name: name.into(),
                mode: Mode::Sets,
                fields: [field.to_string()].into(),
            }]
            .into(),
            images: Default::default(),
            sounds: Default::default(),
        }
    }

    #[test]
    fn the_winner_of_an_overlap_follows_the_new_folder_names() {
        let w = World::new();
        w.add("Alpha", Some("Alpha P"), &[]);
        w.add("Beta", Some("Beta P"), &[]);
        let both = index_setting("ship", "Hornet", "attributes");
        let plugins = [
            OverlapPlugin {
                folder: "Alpha",
                identity: "Alpha P",
                index: &both,
            },
            OverlapPlugin {
                folder: "Beta",
                identity: "Beta P",
                index: &both,
            },
        ];
        // Now Beta (sorts later) wins. Make Alpha load last.
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(&["Beta", "Alpha"]))
            .unwrap();
        let changes = winner_changes(&backend(), &plugins, &plan);
        assert_eq!(changes.len(), 1);
        let c = &changes[0];
        assert_eq!(
            c.item,
            OverlapItem::Object {
                kind: "ship".into(),
                name: "Hornet".into()
            }
        );
        assert_eq!(c.was.as_deref(), Some("Beta P"));
        assert_eq!(c.now.as_deref(), Some("Alpha P"));
        assert_eq!(c.overridden, ["Beta P"]);
        assert!(
            c.reason.contains("Alpha P loads after Beta P"),
            "{}",
            c.reason
        );
        assert!(
            c.reason.contains("020-Alpha") && c.reason.contains("010-Beta"),
            "{}",
            c.reason
        );

        // An order that keeps the same winner reports nothing.
        let same = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(&["Alpha", "Beta"]))
            .unwrap();
        assert!(winner_changes(&backend(), &plugins, &same).is_empty());
    }

    #[test]
    fn plugins_that_do_not_overlap_never_show_up() {
        let w = World::new();
        w.add("Alpha", Some("Alpha P"), &[]);
        w.add("Beta", Some("Beta P"), &[]);
        let a = index_setting("ship", "Hornet", "attributes");
        let b = index_setting("ship", "Falcon", "attributes");
        let plugins = [
            OverlapPlugin {
                folder: "Alpha",
                identity: "Alpha P",
                index: &a,
            },
            OverlapPlugin {
                folder: "Beta",
                identity: "Beta P",
                index: &b,
            },
        ];
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(&["Beta", "Alpha"]))
            .unwrap();
        assert!(winner_changes(&backend(), &plugins, &plan).is_empty());
    }

    // --- fixes from review -----------------------------------------------------

    #[test]
    fn a_recovery_never_runs_in_the_middle_of_a_live_commit() {
        use std::sync::mpsc;
        use std::time::Duration;

        let w = std::sync::Arc::new(World::new());
        for n in ["Alpha", "Beta"] {
            w.add(n, Some(n), &[]);
        }
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(&["Alpha", "Beta"]))
            .unwrap();
        let (in_rename, in_rename_rx) = mpsc::channel::<()>();
        let (go, go_rx) = mpsc::channel::<()>();

        let committer = {
            let w = w.clone();
            std::thread::spawn(move || {
                let paused = std::cell::Cell::new(false);
                let rename = |from: &Path, to: &Path| {
                    fs::rename(from, to)?;
                    if !paused.replace(true) {
                        // The first folder is renamed and the journal is on disk: hold here.
                        in_rename.send(()).unwrap();
                        go_rx.recv().unwrap();
                    }
                    Ok(())
                };
                commit_with(&w.paths(), &plan, GameProcess::NotRunning, &rename).unwrap();
            })
        };
        in_rename_rx.recv().unwrap();
        assert!(w.journal_exists(), "the commit is mid-way");

        // A recovery started now must wait for the commit instead of finishing it behind its back.
        let recoverer = {
            let w = w.clone();
            std::thread::spawn(move || backend().recover(&w.paths(), GameProcess::NotRunning))
        };
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            !recoverer.is_finished(),
            "recovery must wait for the live commit"
        );
        assert_eq!(
            w.folders(),
            ["010-Alpha", "Beta"],
            "it has not touched anything"
        );

        go.send(()).unwrap();
        committer.join().unwrap();
        assert_eq!(recoverer.join().unwrap().unwrap(), Recovery::Nothing);
        assert_eq!(w.folders(), ["010-Alpha", "020-Beta"]);
        assert_eq!(w.records().len(), 2);
        assert!(w.records().contains_key("020-Beta"));
    }

    #[test]
    fn a_folder_that_is_not_the_planned_plugin_any_more_is_not_renamed() {
        let w = World::new();
        w.add("Alpha", Some("Alpha P"), &[]);
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(&["Alpha"]))
            .unwrap();
        // Someone swaps in a different plugin under the same folder name.
        fs::write(
            w.plugins().join("Alpha/plugin.txt"),
            "name \"Something Else\"\n",
        )
        .unwrap();
        let err = backend()
            .commit(&w.paths(), &plan, GameProcess::NotRunning)
            .unwrap_err();
        assert!(matches!(err, LoadOrderError::Changed(f) if f == "Alpha"));
        assert_eq!(w.folders(), ["Alpha"]);
        assert!(!w.journal_exists());
    }

    #[test]
    fn recovery_does_not_finish_a_rename_for_a_plugin_that_was_swapped() {
        let w = World::new();
        for n in ["Alpha", "Beta"] {
            w.add(n, Some(n), &[]);
        }
        crash_after(&w, &["Alpha", "Beta"], 1);
        // While the manager was down, a different plugin took Beta's place.
        fs::write(w.plugins().join("Beta/plugin.txt"), "name \"Not Beta\"\n").unwrap();
        let out = backend()
            .recover(&w.paths(), GameProcess::NotRunning)
            .unwrap();
        assert!(
            matches!(&out, Recovery::RolledBack { reason, .. } if reason.contains("Beta")),
            "{out:?}"
        );
        assert_eq!(w.folders(), ["Alpha", "Beta"]);
    }

    #[test]
    fn when_both_names_have_a_record_recovery_keeps_the_plugins_own() {
        let w = World::new();
        w.add("Alpha", Some("Alpha P"), &[]);
        crash_after(&w, &["Alpha"], 1);
        // Meanwhile an auto-adoption wrote a hand-made record (no download hash) under the new name.
        w.set_record("010-Alpha", "Alpha P", "", None);
        let out = backend()
            .recover(&w.paths(), GameProcess::NotRunning)
            .unwrap();
        assert_eq!(out, Recovery::Finished { renames: 0 });
        let records = w.records();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records["010-Alpha"].sha256, "hash",
            "the downloaded plugin's record won"
        );
        assert_eq!(
            records["010-Alpha"].original_folder.as_deref(),
            Some("Alpha")
        );
    }

    #[test]
    fn an_emptied_journal_replays_nothing() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        write_journal(&w.paths().journal_path(), &[]).unwrap();
        let out = backend()
            .recover(&w.paths(), GameProcess::NotRunning)
            .unwrap();
        assert_eq!(out, Recovery::Finished { renames: 0 });
        assert!(!w.journal_exists());
        assert_eq!(w.folders(), ["Alpha"]);
    }

    #[test]
    fn a_journal_from_a_newer_version_is_left_alone() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        fs::write(w.paths().journal_path(), r#"{"format": 2, "renames": []}"#).unwrap();
        assert!(matches!(
            backend().recover(&w.paths(), GameProcess::NotRunning),
            Err(LoadOrderError::Journal(m)) if m.contains("newer")
        ));
        assert!(w.journal_exists());
    }

    #[test]
    fn the_backup_does_not_replace_the_toggle_backup() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        fs::write(w.config.join("plugins.txt"), "state\n\tNow 1\n").unwrap();
        fs::write(
            w.config.join(game_state::BACKUP_FILE),
            "state\n\tBefore 1\n",
        )
        .unwrap();
        apply_all(&w, &["Alpha"]);
        assert_eq!(
            fs::read_to_string(w.config.join(game_state::BACKUP_FILE)).unwrap(),
            "state\n\tBefore 1\n",
            "the backup of the game's list from before the last toggle is untouched"
        );
        assert_eq!(
            fs::read_to_string(w.config.join(BACKUP_FILE)).unwrap(),
            "state\n\tNow 1\n"
        );
    }

    #[test]
    fn restoring_a_plugin_without_a_name_warns_that_its_identity_changes() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        w.add("Beta", Some("Beta"), &[]);
        apply_all(&w, &["Alpha", "Beta"]);
        // An update dropped Beta's name, so the game now knows it by its folder name.
        fs::write(w.plugins().join("020-Beta/plugin.txt"), "version 2\n").unwrap();
        let plan = backend().plan_restore(&w.paths(), &w.installed()).unwrap();
        assert_eq!(plan.renames.len(), 2);
        assert_eq!(
            plan.identity_changes,
            [("020-Beta".to_string(), "Beta".to_string())]
        );
        // A restore with every plugin named has nothing to warn about.
        fs::write(w.plugins().join("020-Beta/plugin.txt"), "name \"Beta\"\n").unwrap();
        let plan = backend().plan_restore(&w.paths(), &w.installed()).unwrap();
        assert!(plan.identity_changes.is_empty());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn originals_that_differ_only_by_case_can_still_be_restored_where_the_filesystem_allows_it() {
        let w = World::new();
        w.add("foo", Some("Foo Lower"), &[]);
        w.add("Foo", Some("Foo Upper"), &[]);
        apply_all(&w, &["Foo", "foo"]);
        assert_eq!(w.folders(), ["010-Foo", "020-foo"]);
        let plan = backend().plan_restore(&w.paths(), &w.installed()).unwrap();
        backend()
            .commit(&w.paths(), &plan, GameProcess::NotRunning)
            .unwrap();
        assert_eq!(w.folders(), ["Foo", "foo"]);
    }

    #[test]
    fn when_the_adopted_record_is_at_the_old_name_the_plugins_own_still_wins() {
        // The mirror of the recovery case: rolling back, the plugin's own record sits at the new
        // name and a hash-less (adopted) record has appeared at the old one.
        let w = World::new();
        w.add("Alpha", Some("Alpha P"), &[]);
        w.set_record("Alpha", "Alpha P", "", None);
        w.set_record("010-Alpha", "Alpha P", "hash", Some("Alpha"));
        let rename = Rename {
            from: "Alpha".into(),
            to: "010-Alpha".into(),
            identity: "Alpha P".into(),
            from_original: None,
            to_original: Some("Alpha".into()),
        };
        // On disk only the old name exists, so the records follow it.
        sync_records(&w.paths(), &[rename]).unwrap();
        let records = w.records();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records["Alpha"].sha256, "hash",
            "the downloaded plugin's record won"
        );
        assert_eq!(records["Alpha"].original_folder, None);
    }

    #[test]
    fn a_rename_that_finds_its_target_taken_just_before_it_runs_puts_everything_back() {
        let w = World::new();
        for n in ["Alpha", "Beta"] {
            w.add(n, Some(n), &[]);
        }
        let plan = backend()
            .plan_apply(&w.paths(), &w.installed(), &strs(&["Alpha", "Beta"]))
            .unwrap();
        let plugins = w.plugins();
        // While the first folder is being renamed, an (empty) folder appears where the second
        // one is going. On Linux a plain rename would silently replace it.
        let rename = |from: &Path, to: &Path| {
            fs::rename(from, to)?;
            fs::create_dir_all(plugins.join("020-Beta"))
        };
        let err = commit_with(&w.paths(), &plan, GameProcess::NotRunning, &rename).unwrap_err();
        assert!(
            matches!(&err, LoadOrderError::RenameFailed { rolled_back: true, to, .. } if to == "020-Beta"),
            "{err:?}"
        );
        assert_eq!(w.folders(), ["020-Beta", "Alpha", "Beta"]);
        assert!(!w.journal_exists());
    }

    #[test]
    fn names_that_are_not_plain_folder_names_are_never_joined_onto_plugins() {
        let w = World::new();
        w.add("Alpha", Some("Alpha"), &[]);
        // A journal (hand-edited or corrupt) that points outside plugins/.
        let evil = Rename {
            from: "Alpha".into(),
            to: "../outside".into(),
            identity: "Alpha".into(),
            from_original: None,
            to_original: Some("Alpha".into()),
        };
        write_journal(&w.paths().journal_path(), &[evil]).unwrap();
        assert!(matches!(
            backend().recover(&w.paths(), GameProcess::NotRunning),
            Err(LoadOrderError::Journal(m)) if m.contains("plain folder names")
        ));
        assert_eq!(w.folders(), ["Alpha"]);
        assert!(!w.dir_outside_exists());
        fs::remove_file(w.paths().journal_path()).unwrap();

        // A record whose original name is a path: restoring refuses instead of moving anything.
        apply_all(&w, &["Alpha"]);
        w.set_record("010-Alpha", "Alpha", "hash", Some("../elsewhere"));
        let err = backend()
            .plan_restore(&w.paths(), &w.installed())
            .unwrap_err();
        assert!(matches!(err, LoadOrderError::Records(_)), "{err:?}");
        assert_eq!(w.folders(), ["010-Alpha"]);
    }
}
