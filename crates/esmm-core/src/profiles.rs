//! Local profiles: named sets of enabled plugins (decision E).
//!
//! Stored in the manager's app-data dir. A profile records only which plugins are enabled,
//! never versions. Installed plugins are given as their game identities.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::files;
use crate::game_state::effective_enabled;
use crate::plugin_state::PluginStates;
use crate::records::InstallRecords;

pub const DEFAULT_PROFILE: &str = "Default";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Identity of each enabled plugin -> its catalog name, when known, so a missing
    /// plugin can be offered for install.
    pub enabled: BTreeMap<String, Option<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileStore {
    pub active: Option<String>,
    pub profiles: BTreeMap<String, Profile>,
}

/// A plugin a profile enables that isn't installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Missing {
    pub identity: String,
    pub catalog_name: Option<String>,
}

/// Where the live state no longer matches a profile, among installed plugins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Drift {
    pub enabled_but_not_in_profile: BTreeSet<String>,
    pub in_profile_but_disabled: BTreeSet<String>,
}

impl Drift {
    pub fn is_empty(&self) -> bool {
        self.enabled_but_not_in_profile.is_empty() && self.in_profile_but_disabled.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("profile name can't be empty")]
    EmptyName,
    #[error("a profile named {0:?} already exists")]
    DuplicateName(String),
    #[error("no profile named {0:?}")]
    NotFound(String),
    #[error("no profile is active")]
    NoActiveProfile,
    #[error("{0}")]
    InvalidShareFile(String),
    #[error("failed to read or write profiles: {0}")]
    Io(#[from] io::Error),
    #[error("profiles file is corrupt: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn catalog_name_for(records: &InstallRecords, identity: &str) -> Option<String> {
    records
        .values()
        .find(|r| r.identity == identity)
        .map(|r| r.catalog_name.clone())
}

impl Profile {
    /// Match the profile to the current state of installed plugins. Entries for plugins
    /// that aren't installed are kept: they're missing, not drift.
    pub fn update_from(
        &mut self,
        installed: &BTreeSet<String>,
        states: &PluginStates,
        records: &InstallRecords,
    ) {
        for identity in installed {
            if effective_enabled(states, identity) {
                let catalog_name = catalog_name_for(records, identity);
                let entry = self.enabled.entry(identity.clone()).or_default();
                if catalog_name.is_some() {
                    *entry = catalog_name;
                }
            } else {
                self.enabled.remove(identity);
            }
        }
    }
}

/// A profile enabling exactly the installed plugins that are currently enabled.
pub fn snapshot(
    installed: &BTreeSet<String>,
    states: &PluginStates,
    records: &InstallRecords,
) -> Profile {
    let mut profile = Profile::default();
    profile.update_from(installed, states, records);
    profile
}

/// The states to write for `profile`, with an explicit entry for every installed plugin
/// (unlisted means enabled in the game), plus the profile's plugins that aren't installed.
pub fn apply(
    profile: &Profile,
    installed: &BTreeSet<String>,
    states: &PluginStates,
) -> (PluginStates, Vec<Missing>) {
    let mut result = states.clone();
    for identity in installed {
        result.insert(identity.clone(), profile.enabled.contains_key(identity));
    }
    let missing = profile
        .enabled
        .iter()
        .filter(|(identity, _)| !installed.contains(*identity))
        .map(|(identity, catalog_name)| Missing {
            identity: identity.clone(),
            catalog_name: catalog_name.clone(),
        })
        .collect();
    (result, missing)
}

pub fn drift(profile: &Profile, installed: &BTreeSet<String>, states: &PluginStates) -> Drift {
    let mut drift = Drift::default();
    for identity in installed {
        match (
            effective_enabled(states, identity),
            profile.enabled.contains_key(identity),
        ) {
            (true, false) => drift.enabled_but_not_in_profile.insert(identity.clone()),
            (false, true) => drift.in_profile_but_disabled.insert(identity.clone()),
            _ => continue,
        };
    }
    drift
}

/// The version of the share-file layout. Bump it only for a change older apps can't read.
pub const SHARE_FORMAT: u32 = 1;
/// A real profile is a few KB; this only stops a wrong or hostile file being read into memory.
pub const MAX_SHARE_BYTES: u64 = 256 * 1024;
const MAX_SHARE_ENTRIES: usize = 2000;
const MAX_SHARE_TEXT: usize = 200;

/// A profile as saved to a file to share. It names plugins and nothing else: no URLs, no
/// versions (decision E). Anything read from one is looked up in the official catalog by name.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedProfile {
    format: u32,
    name: String,
    enabled: Vec<SharedEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedEntry {
    identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    catalog_name: Option<String>,
}

/// The text of a share file for `profile`, named `name`.
pub fn export_profile(name: &str, profile: &Profile) -> Result<String, ProfileError> {
    let shared = SharedProfile {
        format: SHARE_FORMAT,
        name: name.trim().to_string(),
        enabled: profile
            .enabled
            .iter()
            .map(|(identity, catalog_name)| SharedEntry {
                identity: identity.clone(),
                catalog_name: catalog_name.clone(),
            })
            .collect(),
    };
    Ok(serde_json::to_string_pretty(&shared)?)
}

fn clean_text(raw: &str) -> Option<String> {
    let text = raw.trim();
    let ok = !text.is_empty()
        && text.chars().count() <= MAX_SHARE_TEXT
        && !text.chars().any(char::is_control);
    ok.then(|| text.to_string())
}

/// Reads a share file's text into a suggested profile name and the profile. Strict, because the
/// file comes from someone else: bad size, shape or a newer format are refused with a message
/// a person can act on, and unusable entries are rejected rather than silently dropped.
pub fn parse_shared_profile(text: &str) -> Result<(String, Profile), ProfileError> {
    let bad = |msg: &str| ProfileError::InvalidShareFile(msg.to_string());
    if text.len() as u64 > MAX_SHARE_BYTES {
        return Err(bad("That file is too large to be a profile."));
    }
    let shared: SharedProfile = serde_json::from_str(text)
        .map_err(|_| bad("That isn't a profile file made by Endless Sky Mod Manager."))?;
    if shared.format > SHARE_FORMAT {
        return Err(bad(
            "That profile was made by a newer version of the manager. Update the app and try again.",
        ));
    }
    if shared.enabled.len() > MAX_SHARE_ENTRIES {
        return Err(bad("That profile lists too many plugins to be real."));
    }
    let mut profile = Profile::default();
    for entry in &shared.enabled {
        let identity = clean_text(&entry.identity)
            .ok_or_else(|| bad("That profile contains a plugin entry with an invalid name."))?;
        // A bad catalog name just means "unknown"; the plugin is still listed by identity.
        let catalog_name = entry.catalog_name.as_deref().and_then(clean_text);
        profile.enabled.entry(identity).or_insert(catalog_name);
    }
    let name = clean_text(&shared.name).unwrap_or_else(|| "Imported profile".to_string());
    Ok((name, profile))
}

impl ProfileStore {
    /// Like [`insert`](Self::insert), but a taken name gets " (2)", " (3)"... instead of an
    /// error, since an imported name isn't the user's own choice. Returns the stored name.
    pub fn insert_unique(&mut self, name: &str, profile: Profile) -> Result<String, ProfileError> {
        let base = name.trim();
        if base.is_empty() {
            return Err(ProfileError::EmptyName);
        }
        let mut candidate = base.to_string();
        let mut n = 2;
        while self.profiles.contains_key(&candidate) {
            candidate = format!("{base} ({n})");
            n += 1;
        }
        self.insert(&candidate, profile)
    }

    pub fn load(path: &Path) -> Result<Self, ProfileError> {
        files::load_json(path)
    }

    pub fn save(&self, path: &Path) -> Result<(), ProfileError> {
        files::save_json(path, self)
    }

    /// On first run, snapshot the current state as the active "Default" profile.
    /// Returns whether it was created.
    pub fn ensure_default(
        &mut self,
        installed: &BTreeSet<String>,
        states: &PluginStates,
        records: &InstallRecords,
    ) -> bool {
        if !self.profiles.is_empty() {
            return false;
        }
        self.profiles.insert(
            DEFAULT_PROFILE.to_string(),
            snapshot(installed, states, records),
        );
        self.active = Some(DEFAULT_PROFILE.to_string());
        true
    }

    /// Adds a new profile under `name` (trimmed). Returns the stored name.
    pub fn insert(&mut self, name: &str, profile: Profile) -> Result<String, ProfileError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ProfileError::EmptyName);
        }
        if self.profiles.contains_key(name) {
            return Err(ProfileError::DuplicateName(name.to_string()));
        }
        self.profiles.insert(name.to_string(), profile);
        Ok(name.to_string())
    }

    /// Renames `old` to `new` (trimmed), preserving its contents and active status. A no-op
    /// returning `old` unchanged if `new` trims to the same name.
    pub fn rename(&mut self, old: &str, new: &str) -> Result<String, ProfileError> {
        let new = new.trim();
        if new.is_empty() {
            return Err(ProfileError::EmptyName);
        }
        if new == old {
            return Ok(old.to_string());
        }
        if !self.profiles.contains_key(old) {
            return Err(ProfileError::NotFound(old.to_string()));
        }
        if self.profiles.contains_key(new) {
            return Err(ProfileError::DuplicateName(new.to_string()));
        }
        let profile = self.profiles.remove(old).expect("checked above");
        self.profiles.insert(new.to_string(), profile);
        if self.active.as_deref() == Some(old) {
            self.active = Some(new.to_string());
        }
        Ok(new.to_string())
    }

    pub fn set_active(&mut self, name: &str) -> Result<(), ProfileError> {
        if !self.profiles.contains_key(name) {
            return Err(ProfileError::NotFound(name.to_string()));
        }
        self.active = Some(name.to_string());
        Ok(())
    }

    pub fn active_profile(&self) -> Option<&Profile> {
        self.profiles.get(self.active.as_deref()?)
    }

    pub fn active_profile_mut(&mut self) -> Option<&mut Profile> {
        self.profiles.get_mut(self.active.as_deref()?)
    }

    /// A newly installed plugin joins the active profile, if any, as enabled.
    /// A catalog name already known for it is kept when `catalog_name` is None.
    pub fn on_installed(&mut self, identity: &str, catalog_name: Option<&str>) {
        if let Some(profile) = self.active_profile_mut() {
            let entry = profile.enabled.entry(identity.to_string()).or_default();
            if let Some(name) = catalog_name {
                *entry = Some(name.to_string());
            }
        }
    }

    /// Drift resolution: make the active profile match the current state.
    pub fn update_active(
        &mut self,
        installed: &BTreeSet<String>,
        states: &PluginStates,
        records: &InstallRecords,
    ) -> Result<(), ProfileError> {
        let profile = self
            .active_profile_mut()
            .ok_or(ProfileError::NoActiveProfile)?;
        profile.update_from(installed, states, records);
        Ok(())
    }

    /// Drift resolution: save the current state as a new profile and make it active.
    pub fn save_current_as(
        &mut self,
        name: &str,
        installed: &BTreeSet<String>,
        states: &PluginStates,
        records: &InstallRecords,
    ) -> Result<String, ProfileError> {
        let name = self.insert(name, snapshot(installed, states, records))?;
        self.active = Some(name.clone());
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::InstallRecord;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn states(pairs: &[(&str, bool)]) -> PluginStates {
        pairs.iter().map(|(n, on)| (n.to_string(), *on)).collect()
    }

    fn records() -> InstallRecords {
        let record = InstallRecord {
            catalog_name: "Jimmys-Ship-Emporium".into(),
            folder: "Jimmys-Ship-Emporium".into(),
            identity: "Jimmy's Ship Emporium".into(),
            version: "v0.1.1".into(),
            source_url: "https://example.com/a.zip".into(),
            sha256: "ab".repeat(32),
        };
        InstallRecords::from([(record.folder.clone(), record)])
    }

    fn profile(entries: &[(&str, Option<&str>)]) -> Profile {
        Profile {
            enabled: entries
                .iter()
                .map(|(i, c)| (i.to_string(), c.map(str::to_string)))
                .collect(),
        }
    }

    #[test]
    fn snapshot_takes_effective_enabled_with_catalog_names() {
        let installed = set(&["Jimmy's Ship Emporium", "Off", "Unlisted"]);
        let got = snapshot(
            &installed,
            &states(&[("Off", false), ("Not Installed", true)]),
            &records(),
        );
        assert_eq!(
            got,
            profile(&[
                ("Jimmy's Ship Emporium", Some("Jimmys-Ship-Emporium")),
                ("Unlisted", None),
            ])
        );
    }

    #[test]
    fn apply_lists_every_installed_plugin_and_reports_missing() {
        let p = profile(&[("A", None), ("Gone", Some("Gone-Plugin"))]);
        let installed = set(&["A", "B", "C"]);
        let (result, missing) = apply(&p, &installed, &states(&[("A", false), ("Old", true)]));
        assert_eq!(
            result,
            states(&[("A", true), ("B", false), ("C", false), ("Old", true)])
        );
        assert_eq!(
            missing,
            [Missing {
                identity: "Gone".into(),
                catalog_name: Some("Gone-Plugin".into()),
            }]
        );
    }

    #[test]
    fn drift_ignores_missing_plugins() {
        let p = profile(&[("A", None), ("B", None), ("Gone", None)]);
        let installed = set(&["A", "B", "C", "D"]);
        let current = states(&[("B", false), ("D", false)]);
        let d = drift(&p, &installed, &current);
        assert_eq!(d.enabled_but_not_in_profile, set(&["C"]));
        assert_eq!(d.in_profile_but_disabled, set(&["B"]));

        let (applied, _) = apply(&p, &installed, &current);
        assert!(drift(&p, &installed, &applied).is_empty());
    }

    #[test]
    fn default_profile_only_on_first_run() {
        let installed = set(&["A", "B"]);
        let mut store = ProfileStore::default();
        assert!(store.ensure_default(&installed, &states(&[("B", false)]), &records()));
        assert_eq!(store.active.as_deref(), Some(DEFAULT_PROFILE));
        assert_eq!(store.active_profile(), Some(&profile(&[("A", None)])));

        assert!(!store.ensure_default(&installed, &states(&[]), &records()));
        assert_eq!(store.active_profile(), Some(&profile(&[("A", None)])));
    }

    #[test]
    fn new_installs_join_the_active_profile() {
        let mut store = ProfileStore::default();
        store.on_installed("X", Some("X-Cat"));
        assert!(store.profiles.is_empty());

        store.ensure_default(&set(&[]), &states(&[]), &records());
        store.on_installed("X", Some("X-Cat"));
        assert_eq!(
            store.active_profile(),
            Some(&profile(&[("X", Some("X-Cat"))]))
        );

        store.on_installed("X", None);
        store.on_installed("Y", None);
        assert_eq!(
            store.active_profile(),
            Some(&profile(&[("X", Some("X-Cat")), ("Y", None)])),
            "a known catalog name is not erased"
        );
    }

    #[test]
    fn drift_resolutions() {
        let installed = set(&["A", "B", "C"]);
        let mut store = ProfileStore::default();
        store
            .insert("Main", profile(&[("A", None), ("Gone", Some("G"))]))
            .unwrap();
        store.set_active("Main").unwrap();
        let current = states(&[("A", false)]);

        let name = store
            .save_current_as(" Alt ", &installed, &current, &records())
            .unwrap();
        assert_eq!(name, "Alt");
        assert_eq!(store.active.as_deref(), Some("Alt"));
        assert_eq!(
            store.active_profile(),
            Some(&profile(&[("B", None), ("C", None)]))
        );

        store.set_active("Main").unwrap();
        store
            .update_active(&installed, &current, &records())
            .unwrap();
        assert_eq!(
            store.active_profile(),
            Some(&profile(&[("B", None), ("C", None), ("Gone", Some("G"))])),
            "the missing plugin's entry survives"
        );
        assert!(drift(store.active_profile().unwrap(), &installed, &current).is_empty());
    }

    #[test]
    fn names_are_validated() {
        let mut store = ProfileStore::default();
        assert!(matches!(
            store.insert("  ", Profile::default()),
            Err(ProfileError::EmptyName)
        ));
        store.insert("A", Profile::default()).unwrap();
        assert!(matches!(
            store.insert(" A", Profile::default()),
            Err(ProfileError::DuplicateName(_))
        ));
        assert!(matches!(
            store.set_active("B"),
            Err(ProfileError::NotFound(_))
        ));
        assert!(matches!(
            store.update_active(&set(&[]), &states(&[]), &records()),
            Err(ProfileError::NoActiveProfile)
        ));
    }

    #[test]
    fn rename_preserves_contents_and_active_status() {
        let mut store = ProfileStore::default();
        store.insert("Main", profile(&[("A", None)])).unwrap();
        store.insert("Alt", profile(&[("B", None)])).unwrap();
        store.set_active("Main").unwrap();

        let renamed = store.rename("Main", " Primary ").unwrap();
        assert_eq!(renamed, "Primary");
        assert!(!store.profiles.contains_key("Main"));
        assert_eq!(
            store.active.as_deref(),
            Some("Primary"),
            "active follows the rename"
        );
        assert_eq!(store.active_profile(), Some(&profile(&[("A", None)])));
        assert_eq!(
            store.profiles.get("Alt"),
            Some(&profile(&[("B", None)])),
            "other profiles untouched"
        );
    }

    #[test]
    fn rename_to_its_own_name_is_a_no_op() {
        let mut store = ProfileStore::default();
        store.insert("Main", profile(&[("A", None)])).unwrap();
        assert_eq!(store.rename("Main", " Main ").unwrap(), "Main");
        assert_eq!(store.profiles.len(), 1);
    }

    #[test]
    fn rename_is_validated() {
        let mut store = ProfileStore::default();
        store.insert("Main", Profile::default()).unwrap();
        store.insert("Alt", Profile::default()).unwrap();
        assert!(matches!(
            store.rename("Main", "  "),
            Err(ProfileError::EmptyName)
        ));
        assert!(matches!(
            store.rename("Main", "Alt"),
            Err(ProfileError::DuplicateName(_))
        ));
        assert!(matches!(
            store.rename("Gone", "New"),
            Err(ProfileError::NotFound(_))
        ));
        // rejected renames change nothing
        assert!(store.profiles.contains_key("Main"));
        assert!(store.profiles.contains_key("Alt"));
    }

    #[test]
    fn a_profile_survives_export_and_import() {
        let original = profile(&[("A", Some("A-Cat")), ("B", None)]);
        let text = export_profile("  My run ", &original).unwrap();
        let (name, back) = parse_shared_profile(&text).unwrap();
        assert_eq!(name, "My run");
        assert_eq!(back, original);
    }

    #[test]
    fn import_refuses_files_it_cannot_trust() {
        let invalid = |text: &str| {
            matches!(
                parse_shared_profile(text),
                Err(ProfileError::InvalidShareFile(_))
            )
        };
        assert!(invalid("not json"));
        assert!(invalid("{}"));
        assert!(
            invalid(r#"{"format":99,"name":"x","enabled":[]}"#),
            "newer format"
        );
        assert!(invalid(
            r#"{"format":1,"name":"x","enabled":[{"identity":""}]}"#
        ));
        assert!(invalid(
            r#"{"format":1,"name":"x","enabled":[{"identity":"a b"}]}"#
        ));
        let many: Vec<String> = (0..MAX_SHARE_ENTRIES + 1)
            .map(|i| format!(r#"{{"identity":"p{i}"}}"#))
            .collect();
        assert!(invalid(&format!(
            r#"{{"format":1,"name":"x","enabled":[{}]}}"#,
            many.join(",")
        )));
        assert!(invalid(&" ".repeat(MAX_SHARE_BYTES as usize + 1)));
    }

    #[test]
    fn import_cleans_names_and_ignores_extra_fields() {
        let text = r#"{"format":1,"name":"   ","unknown":"ignored","enabled":[
            {"identity":" A ","catalogName":"A-Cat","url":"https://evil.example/x.zip"},
            {"identity":"A","catalogName":"Other"},
            {"identity":"B","catalogName":""}]}"#;
        let (name, profile) = parse_shared_profile(text).unwrap();
        assert_eq!(name, "Imported profile", "blank names get a default");
        assert_eq!(profile.enabled.len(), 2, "duplicates collapse to the first");
        assert_eq!(profile.enabled["A"].as_deref(), Some("A-Cat"));
        assert_eq!(
            profile.enabled["B"], None,
            "a blank catalog name is unknown"
        );
    }

    #[test]
    fn insert_unique_numbers_a_taken_name() {
        let mut store = ProfileStore::default();
        assert_eq!(store.insert_unique("Run", profile(&[])).unwrap(), "Run");
        assert_eq!(store.insert_unique("Run", profile(&[])).unwrap(), "Run (2)");
        assert_eq!(store.insert_unique("Run", profile(&[])).unwrap(), "Run (3)");
        assert!(matches!(
            store.insert_unique("  ", profile(&[])),
            Err(ProfileError::EmptyName)
        ));
    }

    #[test]
    fn save_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app-data/profiles.json");
        assert_eq!(ProfileStore::load(&path).unwrap(), ProfileStore::default());

        let mut store = ProfileStore::default();
        store.ensure_default(&set(&["A"]), &states(&[]), &records());
        store
            .insert("Other", profile(&[("B", Some("B-Cat"))]))
            .unwrap();
        store.save(&path).unwrap();
        assert_eq!(ProfileStore::load(&path).unwrap(), store);
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }
}
