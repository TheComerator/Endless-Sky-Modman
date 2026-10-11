//! "Give it a name" (`plugin_naming`) together with the rest of the manager: updates must put
//! the added name back (the critical part: an update replaces the whole folder), unless the user
//! took it out by hand; plans for those updates must not show false problems; a requirement on a
//! renamed plugin's old name must be reported, never "satisfied" by a second copy; and the
//! manager's own commits must wait for an unfinished naming. All offline, in temp dirs; a
//! `Fetcher` serves zips built on the fly.
//!
//! This file belongs to the optional load-order feature's naming step, but deliberately does
//! not use `load_order` (so it keeps compiling if the rest of the feature is removed while
//! named plugins still need their name put back). Naming together with the load order is in
//! `tests/load_order_naming.rs`.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use esmm_core::catalog::CatalogEntry;
use esmm_core::download::Downloaded;
use esmm_core::game_state::{self, GameProcess};
use esmm_core::install;
use esmm_core::manager::{self, CommitContext, CommitError, Fetcher, InstalledPlugin, PlanContext};
use esmm_core::plugin_naming::{self, NameChoice};
use esmm_core::plugin_state::PluginStates;
use esmm_core::profiles::{Profile, ProfileStore};
use esmm_core::records::{self, InstallRecords};
use esmm_core::resolve::Issue;
use zip::write::SimpleFileOptions;

/// A zip wrapping one plugin folder; `plugin_txt` is the exact file (`None`: no plugin.txt).
fn make_plugin_zip(path: &Path, wrapper: &str, plugin_txt: Option<&str>) {
    let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
    let opts = SimpleFileOptions::default();
    zip.add_directory(format!("{wrapper}/data/"), opts).unwrap();
    zip.start_file(format!("{wrapper}/data/x.txt"), opts)
        .unwrap();
    zip.write_all(b"ship stuff").unwrap();
    if let Some(text) = plugin_txt {
        zip.start_file(format!("{wrapper}/plugin.txt"), opts)
            .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

fn entry(catalog_name: &str, version: &str) -> CatalogEntry {
    CatalogEntry {
        name: catalog_name.to_string(),
        authors: "Test Author".into(),
        homepage: "https://example.com".into(),
        license: "MIT".into(),
        version: version.into(),
        short_description: String::new(),
        description: None,
        url: format!("https://example.com/{catalog_name}.zip"),
        icon_url: None,
        autoupdate: None,
    }
}

#[derive(Default)]
struct TestFetcher {
    files: HashMap<String, PathBuf>,
}

impl Fetcher for TestFetcher {
    fn fetch(
        &self,
        entry: &CatalogEntry,
        dest: &Path,
        _progress: &mut dyn FnMut(u64, Option<u64>),
        _cancel: &AtomicBool,
    ) -> Result<Downloaded, String> {
        let src = self.files.get(&entry.url).ok_or("no fixture")?;
        let bytes = fs::read(src).map_err(|e| e.to_string())?;
        fs::write(dest, &bytes).map_err(|e| e.to_string())?;
        Ok(Downloaded {
            bytes: bytes.len() as u64,
            sha256: "test-sha".into(),
        })
    }
}

struct World {
    dir: tempfile::TempDir,
    config: PathBuf,
    records_path: PathBuf,
    profiles_path: PathBuf,
    catalog: Vec<CatalogEntry>,
    fetcher: TestFetcher,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("endless-sky");
        fs::create_dir_all(install::plugins_dir(&config)).unwrap();
        World {
            records_path: dir.path().join("app-data/records.json"),
            profiles_path: dir.path().join("app-data/profiles.json"),
            catalog: Vec::new(),
            fetcher: TestFetcher::default(),
            config,
            dir,
        }
    }

    /// Publishes `version` of a catalog plugin whose plugin.txt is exactly `plugin_txt`.
    fn publish(&mut self, catalog_name: &str, version: &str, plugin_txt: Option<&str>) {
        let e = entry(catalog_name, version);
        let zip = self
            .dir
            .path()
            .join(format!("{catalog_name}-{version}.zip"));
        make_plugin_zip(&zip, &format!("{catalog_name}-{version}"), plugin_txt);
        self.fetcher.files.insert(e.url.clone(), zip);
        self.catalog.retain(|c| c.name != catalog_name);
        self.catalog.push(e);
    }

    fn plugins_dir(&self) -> PathBuf {
        install::plugins_dir(&self.config)
    }

    fn records(&self) -> InstallRecords {
        records::load(&self.records_path).unwrap()
    }

    fn states(&self) -> PluginStates {
        game_state::read_plugin_states(&self.config).unwrap()
    }

    fn installed(&self) -> Vec<InstalledPlugin> {
        manager::scan_installed(&self.plugins_dir(), &self.records()).unwrap()
    }

    fn identity_of(&self, folder: &str) -> String {
        self.installed()
            .into_iter()
            .find(|p| p.folder == folder)
            .unwrap()
            .identity
    }

    fn folders(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.plugins_dir())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn plugin_txt(&self, folder: &str) -> String {
        fs::read_to_string(self.plugins_dir().join(folder).join("plugin.txt")).unwrap()
    }

    fn commit_ctx(&self) -> CommitContext<'_> {
        CommitContext::new(&self.config, &self.records_path, &self.profiles_path)
    }

    fn with_ctx<T>(&self, f: impl FnOnce(&PlanContext) -> T) -> T {
        let installed = self.installed();
        let records = self.records();
        let states = self.states();
        f(&PlanContext {
            catalog: &self.catalog,
            installed: &installed,
            records: &records,
            states: &states,
            game_version: None,
            config_dir: &self.config,
            fetcher: &self.fetcher,
        })
    }

    fn install(&self, catalog_name: &str) {
        let entry = self
            .catalog
            .iter()
            .find(|e| e.name == catalog_name)
            .unwrap()
            .clone();
        let plan = self.with_ctx(|ctx| manager::plan_install(&entry, ctx));
        assert!(plan.issues.is_empty(), "{:?}", plan.issues);
        manager::commit(plan, &self.commit_ctx(), false).unwrap();
    }

    fn naming_paths(&self) -> plugin_naming::Paths<'_> {
        plugin_naming::Paths {
            config_dir: &self.config,
            records_path: &self.records_path,
            profiles_path: &self.profiles_path,
        }
    }

    fn name(&self, folder: &str, choice: NameChoice) {
        let plan = plugin_naming::plan_name(
            &self.naming_paths(),
            &self.installed(),
            &self.states(),
            folder,
            &choice,
        )
        .unwrap();
        plugin_naming::commit_name(&self.naming_paths(), &plan, GameProcess::NotRunning).unwrap();
    }

    fn set_enabled(&self, identity: &str, on: bool) {
        let mut states = self.states();
        states.insert(identity.to_string(), on);
        game_state::write_plugin_states(&self.config, &states, GameProcess::NotRunning).unwrap();
    }

    fn update(&self, folder: &str) {
        let plan = self
            .with_ctx(|ctx| manager::plan_update(folder, ctx))
            .unwrap();
        assert!(plan.issues.is_empty(), "false problems: {:?}", plan.issues);
        manager::commit_update(plan, &self.commit_ctx(), false).unwrap();
    }
}

const NAMELESS_V1: &str = "about \"First version\"\nversion 1\n";
const NAMELESS_V2: &str = "about \"Second version\"\nversion 2\n";

/// Foo (no name in its plugin.txt) and Bar (named) installed from the catalog.
fn world_with_foo() -> World {
    let mut w = World::new();
    w.publish("Foo", "1", Some(NAMELESS_V1));
    w.publish("Bar", "1", Some("name \"Bar\"\n"));
    w.install("Foo");
    w.install("Bar");
    assert_eq!(w.identity_of("Foo"), "Foo");
    w
}

// --- updates put the name back -------------------------------------------------------------

#[test]
fn an_update_puts_the_added_name_back() {
    let mut w = world_with_foo();
    w.name("Foo", NameChoice::FolderName);
    w.set_enabled("Foo", false);
    w.publish("Foo", "2", Some(NAMELESS_V2));
    w.update("Foo");

    assert_eq!(w.folders(), ["Bar", "Foo"], "no second copy");
    assert_eq!(
        w.plugin_txt("Foo"),
        format!("name \"Foo\"\n{NAMELESS_V2}"),
        "the new version, with the name line back on top"
    );
    assert_eq!(w.identity_of("Foo"), "Foo");
    assert_eq!(w.states().get("Foo"), Some(&false));
    let record = &w.records()["Foo"];
    assert_eq!(record.version, "2");
    assert_eq!(record.added_name.as_deref(), Some("Foo"));
    // The backup now holds the new version's file as it came.
    let backup = plugin_naming::backup_dir(&w.records_path).join("Foo.plugin.txt");
    assert_eq!(fs::read_to_string(backup).unwrap(), NAMELESS_V2);
}

#[test]
fn an_update_of_a_plugin_with_a_typed_name_plans_without_false_problems() {
    let mut w = world_with_foo();
    // Dep requires Foo by the typed name.
    w.publish(
        "Dep",
        "1",
        Some("name \"Dep\"\ndependencies\n\trequires\n\t\t\"Foo Expanded\"\n"),
    );
    w.name("Foo", NameChoice::Typed("Foo Expanded".into()));
    w.install("Dep");
    assert_eq!(w.identity_of("Foo"), "Foo Expanded");

    w.publish("Foo", "2", None); // the new version has no plugin.txt at all
    let plan = w.with_ctx(|ctx| manager::plan_update("Foo", ctx)).unwrap();
    assert_eq!(
        plan.issues,
        Vec::<Issue>::new(),
        "Dep's requirement still met"
    );
    manager::commit_update(plan, &w.commit_ctx(), false).unwrap();

    assert_eq!(w.plugin_txt("Foo"), "name \"Foo Expanded\"\n");
    assert_eq!(w.identity_of("Foo"), "Foo Expanded");
    assert_eq!(
        w.records()["Foo"].added_name.as_deref(),
        Some("Foo Expanded")
    );
    let backup = plugin_naming::backup_dir(&w.records_path).join("Foo.no-plugin-txt");
    assert!(backup.exists());
    assert!(
        !plugin_naming::backup_dir(&w.records_path)
            .join("Foo.plugin.txt")
            .exists(),
        "only the backup for the current version is kept"
    );
}

#[test]
fn update_all_puts_every_added_name_back() {
    let mut w = world_with_foo();
    w.publish("Baz", "1", None);
    w.install("Baz");
    w.name("Foo", NameChoice::FolderName);
    w.name("Baz", NameChoice::Typed("Baz Renamed".into()));
    w.publish("Foo", "2", Some(NAMELESS_V2));
    w.publish("Baz", "2", None);

    let plans = w.with_ctx(|ctx| {
        vec![
            manager::plan_update("Foo", ctx).unwrap(),
            manager::plan_update("Baz", ctx).unwrap(),
        ]
    });
    assert!(plans.iter().all(|p| p.issues.is_empty()));
    manager::commit_update_all(plans, &w.commit_ctx(), false).unwrap();
    assert_eq!(w.identity_of("Foo"), "Foo");
    assert_eq!(w.identity_of("Baz"), "Baz Renamed");
    let records = w.records();
    assert_eq!(records["Foo"].added_name.as_deref(), Some("Foo"));
    assert_eq!(records["Baz"].added_name.as_deref(), Some("Baz Renamed"));
}

#[test]
fn when_the_new_version_names_itself_the_added_name_is_dropped() {
    let mut w = world_with_foo();
    w.name("Foo", NameChoice::Typed("Mine".into()));
    w.publish("Foo", "2", Some("name \"Upstream Foo\"\nversion 2\n"));
    // Planned under the new version's own name, as the commit will install it.
    let plan = w.with_ctx(|ctx| manager::plan_update("Foo", ctx)).unwrap();
    manager::commit_update(plan, &w.commit_ctx(), true).unwrap();

    assert_eq!(
        w.plugin_txt("Foo"),
        "name \"Upstream Foo\"\nversion 2\n",
        "nothing is added"
    );
    assert_eq!(w.identity_of("Foo"), "Upstream Foo");
    let record = &w.records()["Foo"];
    assert_eq!(record.added_name, None);
    assert_eq!(record.identity, "Upstream Foo");
}

#[test]
fn a_new_version_with_a_blank_name_line_is_a_problem_in_the_plan_not_a_surprise_at_commit() {
    let mut w = world_with_foo();
    w.name("Foo", NameChoice::FolderName);
    w.publish("Foo", "2", Some("name \"\"\nversion 2\n"));
    let plan = w.with_ctx(|ctx| manager::plan_update("Foo", ctx)).unwrap();
    assert!(
        plan.issues.contains(&Issue::AddedNameCannotBeKept {
            folder: "Foo".into(),
            name: "Foo".into()
        }),
        "{:?}",
        plan.issues
    );
    let err = manager::commit_update(plan, &w.commit_ctx(), false).unwrap_err();
    assert!(matches!(err, CommitError::Blocked(_)), "{err}");
    assert_eq!(
        w.plugin_txt("Foo"),
        format!("name \"Foo\"\n{NAMELESS_V1}"),
        "the installed copy is untouched"
    );

    // Going ahead anyway installs the new version as it is, without the name.
    let plan = w.with_ctx(|ctx| manager::plan_update("Foo", ctx)).unwrap();
    manager::commit_update(plan, &w.commit_ctx(), true).unwrap();
    assert_eq!(w.plugin_txt("Foo"), "name \"\"\nversion 2\n");
    assert_eq!(w.records()["Foo"].added_name, None);
}

#[test]
fn a_typed_name_carries_profiles_and_a_disabled_state_through_an_update() {
    let mut w = world_with_foo();
    w.set_enabled("Foo", false);
    let mut store = ProfileStore::default();
    let mut main = Profile::default();
    main.enabled.insert("Foo".into(), Some("Foo".into()));
    main.enabled.insert("Bar".into(), Some("Bar".into()));
    store.insert("Main", main).unwrap();
    store.active = Some("Main".into());
    store.save(&w.profiles_path).unwrap();

    w.name("Foo", NameChoice::Typed("Foo Expanded".into()));
    w.publish("Foo", "2", Some(NAMELESS_V2));
    w.update("Foo");

    assert_eq!(w.states().get("Foo Expanded"), Some(&false));
    assert_eq!(w.states().get("Foo"), None);
    let profile = &ProfileStore::load(&w.profiles_path).unwrap().profiles["Main"];
    assert!(profile.enabled.contains_key("Foo Expanded"));
    assert!(!profile.enabled.contains_key("Foo"));
}

// --- the user took the name out (or changed it) by hand ------------------------------------

/// Puts `text` in Foo's plugin.txt, as a user editing it (or putting the backup back) would.
fn edit_by_hand(w: &World, folder: &str, text: &str) {
    fs::write(w.plugins_dir().join(folder).join("plugin.txt"), text).unwrap();
}

#[test]
fn a_name_removed_by_hand_is_not_put_back_and_the_plan_agrees() {
    let mut w = world_with_foo();
    w.name("Foo", NameChoice::Typed("Foo Expanded".into()));
    // The user puts the backup back: the plugin is "Foo" again.
    let backup = plugin_naming::backup_dir(&w.records_path).join("Foo.plugin.txt");
    edit_by_hand(&w, "Foo", &fs::read_to_string(backup).unwrap());
    assert_eq!(w.identity_of("Foo"), "Foo");

    w.publish("Foo", "2", Some(NAMELESS_V2));
    w.update("Foo"); // asserts the plan has no problems
    assert_eq!(w.plugin_txt("Foo"), NAMELESS_V2, "no name written back");
    assert_eq!(w.identity_of("Foo"), "Foo");
    let record = &w.records()["Foo"];
    assert_eq!(record.added_name, None, "the hand edit stands");
    assert_eq!(record.identity, "Foo");
}

#[test]
fn a_name_changed_by_hand_is_not_overwritten_even_through_update_all() {
    let mut w = world_with_foo();
    w.publish("Baz", "1", None);
    w.install("Baz");
    w.name("Foo", NameChoice::FolderName);
    w.name("Baz", NameChoice::FolderName);
    edit_by_hand(&w, "Foo", "name \"My Own Foo\"\nversion 1\n");
    w.publish("Foo", "2", Some(NAMELESS_V2));
    w.publish("Baz", "2", None);

    let plans = w.with_ctx(|ctx| {
        vec![
            manager::plan_update("Foo", ctx).unwrap(),
            manager::plan_update("Baz", ctx).unwrap(),
        ]
    });
    assert!(plans.iter().all(|p| p.issues.is_empty()));
    manager::commit_update_all(plans, &w.commit_ctx(), false).unwrap();
    // Foo's hand-chosen name was in the old copy only; the new version has none, and the
    // manager does not impose the name it once added.
    assert_eq!(w.plugin_txt("Foo"), NAMELESS_V2);
    assert_eq!(w.records()["Foo"].added_name, None);
    // Baz was untouched by hand, so its name is put back as usual.
    assert_eq!(w.identity_of("Baz"), "Baz");
    assert_eq!(w.records()["Baz"].added_name.as_deref(), Some("Baz"));
}

// --- requirements on the old name ------------------------------------------------------------

#[test]
fn a_plugin_that_requires_the_old_name_is_reported_and_no_second_copy_is_installed() {
    // Foo was given another name; Dep still requires "Foo". The catalog entry "Foo" is the very
    // plugin that was renamed, so installing Dep must not treat reinstalling it as a fix.
    let mut w = world_with_foo();
    w.name("Foo", NameChoice::Typed("Foo Deluxe".into()));
    w.set_enabled("Foo Deluxe", false);
    w.publish(
        "Dep",
        "1",
        Some("name \"Dep\"\ndependencies\n\trequires\n\t\tFoo\n"),
    );
    let dep = w.catalog.iter().find(|e| e.name == "Dep").unwrap().clone();
    let plan = w.with_ctx(|ctx| manager::plan_install(&dep, ctx));
    assert!(
        plan.issues.contains(&Issue::IdentityMismatch {
            expected: "Foo".into(),
            catalog_name: "Foo".into(),
            actual: "Foo Deluxe".into(),
        }),
        "{:?}",
        plan.issues
    );
    // Going ahead installs Dep only: Foo is not reinstalled, and stays switched off.
    manager::commit(plan, &w.commit_ctx(), true).unwrap();
    assert_eq!(w.folders(), ["Bar", "Dep", "Foo"]);
    assert_eq!(w.identity_of("Foo"), "Foo Deluxe");
    assert_eq!(w.states().get("Foo Deluxe"), Some(&false));
    assert_eq!(w.states().get("Foo"), None);
    assert_eq!(w.records()["Foo"].version, "1", "not reinstalled");
}

// --- commits wait for an unfinished naming ---------------------------------------------------

#[test]
fn the_managers_own_commits_wait_while_a_naming_or_renaming_is_unfinished() {
    let w = world_with_foo();
    for journal in manager::UNFINISHED_CHANGE_JOURNALS {
        let path = w.records_path.with_file_name(journal);
        fs::write(&path, "{}").unwrap();
        let plan = w.with_ctx(|ctx| manager::plan_disable("Bar", ctx));
        let err = manager::commit_disable(plan, &w.commit_ctx(), false).unwrap_err();
        assert!(matches!(err, CommitError::Unfinished), "{err}");
        assert_eq!(
            w.states().get("Bar"),
            Some(&true),
            "still on: nothing written"
        );
        fs::remove_file(&path).unwrap();
    }
    assert_eq!(
        manager::UNFINISHED_CHANGE_JOURNALS[1],
        plugin_naming::JOURNAL_FILE
    );
    let plan = w.with_ctx(|ctx| manager::plan_disable("Bar", ctx));
    manager::commit_disable(plan, &w.commit_ctx(), false).unwrap();
    assert_eq!(w.states().get("Bar"), Some(&false));
}
