//! Overlap warnings for the plan-review dialog (the shell's side of `esmm_core::overlap`).
//!
//! A plan describes a change: plugins to install, enable, update or switch on through a
//! profile. This module works out which plugins would be *enabled and loaded* before the change
//! and after it, reads each one's index (from the cache under the app-data folder; a downloaded
//! plugin that isn't installed yet is read straight from its staging folder), and reports only
//! the overlaps the change creates, grouped by plugin pair. The result is a heads-up, never a
//! block.
//!
//! Plugins are indexed lazily: only the ones a plan needs are read, and once read they stay in
//! the cache, so a plugin you are about to enable (even a disabled one) is checked quickly.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use esmm_core::game_state;
use esmm_core::manager::{InstalledPlugin, PlanStep};
use esmm_core::overlap::{self, PluginEntry, PluginIndex};
use esmm_core::plugin_state::PluginStates;

use crate::views::OverlapPairView;

/// One plugin as it would be loaded.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// Shown to the user (the `plugin.txt` name, else the folder name).
    pub identity: String,
    /// Sets the load order; the game sorts plugin folders by this name.
    pub folder: String,
    /// Where to read it from.
    pub dir: PathBuf,
    /// A downloaded plugin that is not installed yet: read directly, never cached, because it
    /// may be a different version from the cached installed copy of the same folder.
    pub staged: bool,
}

/// The scan was cancelled before it finished.
#[derive(Debug)]
pub struct Cancelled;

/// What a plan would load before and after it is committed.
#[derive(Debug, Clone)]
pub struct ScanInput {
    cache_dir: PathBuf,
    before: Vec<Candidate>,
    after: Vec<Candidate>,
}

/// The plugins loaded under `before` and under `after`, where `after` also replaces the
/// installed copy of each `staged` plugin's folder with the downloaded one and adds new ones.
/// Disabled plugins are left out of both lists: they aren't loaded, so they can't clash.
pub fn input(
    cache_dir: &Path,
    plugins_dir: &Path,
    installed: &[InstalledPlugin],
    before: &PluginStates,
    after: &PluginStates,
    staged: Vec<Candidate>,
) -> ScanInput {
    let loaded = |states: &PluginStates| -> Vec<Candidate> {
        installed
            .iter()
            .filter(|p| game_state::effective_enabled(states, &p.identity))
            .map(|p| Candidate {
                identity: p.identity.clone(),
                folder: p.folder.clone(),
                dir: plugins_dir.join(&p.folder),
                staged: false,
            })
            .collect()
    };
    let mut after_list = loaded(after);
    for s in staged {
        after_list.retain(|c| c.folder != s.folder);
        after_list.push(s);
    }
    ScanInput {
        cache_dir: cache_dir.to_path_buf(),
        before: loaded(before),
        after: after_list,
    }
}

/// Applies a plan's steps to `states`, returning the resulting states and the downloaded
/// plugins that end up enabled (a new install is enabled unless a later step disables it).
pub fn apply_steps(states: &PluginStates, steps: &[PlanStep]) -> (PluginStates, Vec<Candidate>) {
    let mut out = states.clone();
    let mut staged = Vec::new();
    let mut disabled = HashSet::new();
    for step in steps {
        match step {
            PlanStep::Install(plugin) => staged.push(Candidate {
                identity: plugin.identity.clone(),
                folder: plugin.folder.clone(),
                dir: plugin.root().to_path_buf(),
                staged: true,
            }),
            PlanStep::Enable(identity) => {
                out.insert(identity.clone(), true);
                disabled.remove(identity);
            }
            PlanStep::Disable(identity) => {
                out.insert(identity.clone(), false);
                disabled.insert(identity.clone());
            }
        }
    }
    staged.retain(|c| !disabled.contains(&c.identity));
    (out, staged)
}

/// The candidates that could be read, as the comparison wants them.
fn entries<'a>(
    list: &'a [Candidate],
    indexes: &'a HashMap<(String, bool), PluginIndex>,
) -> Vec<PluginEntry<'a>> {
    list.iter()
        .filter_map(|c| {
            indexes
                .get(&(c.folder.clone(), c.staged))
                .map(|index| PluginEntry {
                    folder: &c.folder,
                    index,
                })
        })
        .collect()
}

impl ScanInput {
    /// The plan now also disables `identity` (a conflict resolved by switching it off): it no
    /// longer counts as loaded afterwards.
    pub fn without(&mut self, identity: &str) {
        self.after.retain(|c| c.identity != identity);
    }

    /// Reads whatever isn't cached yet and compares. A plugin that can't be read is skipped
    /// (and logged) rather than failing the plan: a warning is a courtesy, not a requirement.
    pub fn run(&self, cancel: &AtomicBool) -> Result<Vec<OverlapPairView>, Cancelled> {
        let started = Instant::now();
        let mut indexes: HashMap<(String, bool), PluginIndex> = HashMap::new();
        for c in self.before.iter().chain(&self.after) {
            if cancel.load(Ordering::Relaxed) {
                return Err(Cancelled);
            }
            let key = (c.folder.clone(), c.staged);
            if indexes.contains_key(&key) {
                continue;
            }
            let result = if c.staged {
                overlap::index_plugin(&c.dir)
            } else {
                overlap::load_or_index(&c.dir, &self.cache_dir.join(format!("{}.json", c.folder)))
            };
            match result {
                Ok(index) => {
                    indexes.insert(key, index);
                }
                Err(error) => {
                    tracing::warn!(plugin = %c.folder, %error, "couldn't read plugin for the overlap check");
                }
            }
        }
        let before = overlap::find_overlaps(&entries(&self.before, &indexes));
        let after = overlap::find_overlaps(&entries(&self.after, &indexes));

        let names: HashMap<&str, &str> = self
            .before
            .iter()
            .chain(&self.after)
            .map(|c| (c.folder.as_str(), c.identity.as_str()))
            .collect();
        let name = |folder: &str| names.get(folder).copied().unwrap_or(folder).to_string();
        let views: Vec<OverlapPairView> = overlap::new_overlaps(&before, &after)
            .into_iter()
            .map(|p| OverlapPairView {
                winner: name(&p.winner),
                loser: name(&p.loser),
                objects: p.objects as u32,
                images: p.images as u32,
                sounds: p.sounds as u32,
                examples: p.examples,
                maybe_harmless: p.maybe_harmless,
            })
            .collect();
        tracing::info!(
            plugins = indexes.len(),
            warnings = views.len(),
            millis = started.elapsed().as_millis() as u64,
            "overlap check finished"
        );
        Ok(views)
    }
}
