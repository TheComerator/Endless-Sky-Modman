//! "Give it a name" (`plugin_naming`) together with the load-order feature (`load_order`): a
//! named plugin is ordered and restored with its identity unchanged, an update puts the name back
//! in a numbered folder and is planned under that name, a requirement on a renamed plugin's old
//! name never installs a second copy beside its numbered folder, and a plugin whose name was
//! taken out after it was numbered must be restored before it can be named again. All offline,
//! in temp dirs; a `Fetcher` serves zips built on the fly.
//!
//! Belongs to both parts of the optional load-order feature: delete it if either is removed.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use esmm_core::catalog::CatalogEntry;
use esmm_core::download::Downloaded;
use esmm_core::game_state::{self, GameProcess};
use esmm_core::install;
use esmm_core::load_order::{self, LoadOrderBackend};
use esmm_core::manager::{self, CommitContext, Fetcher, InstalledPlugin, PlanContext, PlanStep};
use esmm_core::plugin_naming::{self, NameChoice, NamingError};
use esmm_core::plugin_state::PluginStates;
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
        names.sort_by(|a, b| load_order::game_order(a, b));
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

    fn apply_order(&self, wanted: &[&str]) {
        let backend = load_order::default_backend();
        let paths = load_order::Paths::new(&self.config, &self.records_path);
        let wanted: Vec<String> = wanted.iter().map(|s| s.to_string()).collect();
        let plan = backend
            .plan_apply(&paths, &self.installed(), &wanted)
            .unwrap();
        backend
            .commit(&paths, &plan, GameProcess::NotRunning)
            .unwrap();
    }

    fn restore_order(&self) {
        let backend = load_order::default_backend();
        let paths = load_order::Paths::new(&self.config, &self.records_path);
        let plan = backend.plan_restore(&paths, &self.installed()).unwrap();
        assert!(
            plan.identity_changes.is_empty(),
            "{:?}",
            plan.identity_changes
        );
        backend
            .commit(&paths, &plan, GameProcess::NotRunning)
            .unwrap();
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

#[test]
fn a_named_plugin_can_be_ordered_and_restored_with_its_identity_unchanged() {
    let w = world_with_foo();
    let layout = load_order::default_backend().layout(&w.installed());
    assert!(layout.ordered.iter().all(|p| p.folder != "Foo"), "left out");

    w.name("Foo", NameChoice::FolderName);
    let layout = load_order::default_backend().layout(&w.installed());
    let orderable: Vec<&str> = layout.ordered.iter().map(|p| p.folder.as_str()).collect();
    assert_eq!(orderable, ["Bar", "Foo"]);

    w.apply_order(&["Foo", "Bar"]);
    assert_eq!(w.folders(), ["010-Foo", "020-Bar"]);
    assert_eq!(w.identity_of("010-Foo"), "Foo", "the game still sees Foo");
    let record = &w.records()["010-Foo"];
    assert_eq!(record.original_folder.as_deref(), Some("Foo"));
    assert_eq!(record.added_name.as_deref(), Some("Foo"));

    w.restore_order();
    assert_eq!(w.folders(), ["Bar", "Foo"]);
    assert_eq!(w.identity_of("Foo"), "Foo");
    let record = &w.records()["Foo"];
    assert_eq!(record.original_folder, None);
    assert_eq!(record.added_name.as_deref(), Some("Foo"));
}

#[test]
fn an_update_puts_the_added_name_back_in_a_numbered_folder() {
    let mut w = world_with_foo();
    w.name("Foo", NameChoice::FolderName);
    w.apply_order(&["Bar", "Foo"]);
    assert_eq!(w.folders(), ["010-Bar", "020-Foo"]);
    w.set_enabled("Foo", false);

    w.publish("Foo", "2", Some(NAMELESS_V2));
    w.update("020-Foo");

    assert_eq!(w.folders(), ["010-Bar", "020-Foo"], "no second copy");
    assert_eq!(
        w.plugin_txt("020-Foo"),
        format!("name \"Foo\"\n{NAMELESS_V2}")
    );
    // Without the name it would be "020-Foo" now, and its off switch would no longer apply.
    assert_eq!(w.identity_of("020-Foo"), "Foo");
    assert_eq!(w.states().get("Foo"), Some(&false));
    assert_eq!(w.states().get("020-Foo"), None);
    let record = &w.records()["020-Foo"];
    assert_eq!(record.version, "2");
    assert_eq!(record.identity, "Foo");
    assert_eq!(record.added_name.as_deref(), Some("Foo"));
    assert_eq!(record.original_folder.as_deref(), Some("Foo"));
    let backup = plugin_naming::backup_dir(&w.records_path).join("Foo.plugin.txt");
    assert_eq!(fs::read_to_string(backup).unwrap(), NAMELESS_V2);
}

#[test]
fn an_update_in_a_numbered_folder_is_planned_under_the_added_name_not_the_folder_name() {
    // Without the name, the new copy in "030-Foo" would look like a plugin called "030-Foo",
    // and Dep's requirement on "Foo" would look unmet.
    let mut w = world_with_foo();
    w.publish(
        "Dep",
        "1",
        Some("name \"Dep\"\ndependencies\n\trequires\n\t\tFoo\n"),
    );
    w.install("Dep");
    w.name("Foo", NameChoice::FolderName);
    w.apply_order(&["Bar", "Dep", "Foo"]);
    assert_eq!(w.folders(), ["010-Bar", "020-Dep", "030-Foo"]);

    w.publish("Foo", "2", Some(NAMELESS_V2));
    let plan = w
        .with_ctx(|ctx| manager::plan_update("030-Foo", ctx))
        .unwrap();
    assert_eq!(plan.issues, Vec::<Issue>::new());
    manager::commit_update(plan, &w.commit_ctx(), false).unwrap();
    assert_eq!(w.identity_of("030-Foo"), "Foo");
}

/// Foo given the typed name "Foo Deluxe", switched off, and numbered `020-Foo`.
fn renamed_and_numbered() -> World {
    let w = world_with_foo();
    w.name("Foo", NameChoice::Typed("Foo Deluxe".into()));
    w.set_enabled("Foo Deluxe", false);
    w.apply_order(&["Bar", "Foo"]);
    assert_eq!(w.folders(), ["010-Bar", "020-Foo"]);
    w
}

fn assert_one_switched_off_copy(w: &World) {
    assert_eq!(
        w.folders(),
        ["010-Bar", "020-Foo", "Dep"],
        "no second copy of Foo"
    );
    let records = w.records();
    assert_eq!(
        records.values().filter(|r| r.catalog_name == "Foo").count(),
        1,
        "no second record for Foo"
    );
    assert_eq!(w.states().get("Foo Deluxe"), Some(&false), "still off");
    assert_eq!(w.states().get("Foo"), None, "no switched-on copy");
}

#[test]
fn installing_a_plugin_that_requires_the_old_name_never_adds_a_second_copy_beside_a_numbered_folder()
 {
    let mut w = renamed_and_numbered();
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
    assert!(
        plan.steps
            .iter()
            .all(|s| !matches!(s, PlanStep::Install(staged) if staged.catalog_name == "Foo")),
        "Foo is not staged at all"
    );
    // Even going ahead anyway.
    manager::commit(plan, &w.commit_ctx(), true).unwrap();
    assert_one_switched_off_copy(&w);
}

#[test]
fn updating_a_plugin_that_now_requires_the_old_name_never_adds_a_second_copy_either() {
    let mut w = renamed_and_numbered();
    w.publish("Dep", "1", Some("name \"Dep\"\n"));
    w.install("Dep");
    w.apply_order(&["010-Bar", "020-Foo", "Dep"]);
    assert_eq!(w.folders(), ["010-Bar", "020-Foo", "030-Dep"]);
    // Dep's new version requires Foo by its old name.
    w.publish(
        "Dep",
        "2",
        Some("name \"Dep\"\ndependencies\n\trequires\n\t\tFoo\n"),
    );
    let plan = w
        .with_ctx(|ctx| manager::plan_update("030-Dep", ctx))
        .unwrap();
    assert!(plan.new_requirements().is_empty(), "nothing staged for Foo");
    assert!(
        plan.issues.contains(&Issue::IdentityMismatch {
            expected: "Foo".into(),
            catalog_name: "Foo".into(),
            actual: "Foo Deluxe".into(),
        }),
        "{:?}",
        plan.issues
    );
    manager::commit_update(plan, &w.commit_ctx(), true).unwrap();
    assert_eq!(w.folders(), ["010-Bar", "020-Foo", "030-Dep"]);
    assert_eq!(
        w.records()
            .values()
            .filter(|r| r.catalog_name == "Foo")
            .count(),
        1
    );
    assert_eq!(w.states().get("Foo Deluxe"), Some(&false));
    assert_eq!(w.states().get("Foo"), None);
}

#[test]
fn a_plugin_whose_name_was_taken_out_after_numbering_must_be_restored_before_naming_again() {
    let w = world_with_foo();
    w.name("Foo", NameChoice::FolderName);
    w.apply_order(&["Bar", "Foo"]);
    // The user takes the name out by hand: now it is nameless in a numbered folder.
    fs::write(w.plugins_dir().join("020-Foo/plugin.txt"), NAMELESS_V1).unwrap();
    let layout = load_order::default_backend().layout(&w.installed());
    let excluded = layout
        .excluded
        .iter()
        .find(|e| e.folder == "020-Foo")
        .unwrap();
    assert!(excluded.still_renamed);
    let err = plugin_naming::plan_name(
        &w.naming_paths(),
        &w.installed(),
        &w.states(),
        "020-Foo",
        &NameChoice::FolderName,
    )
    .unwrap_err();
    assert!(matches!(err, NamingError::RestoreFirst { .. }), "{err}");

    // After "Restore original names" it can be named again, as "Foo".
    let backend = load_order::default_backend();
    let paths = load_order::Paths::new(&w.config, &w.records_path);
    let plan = backend.plan_restore(&paths, &w.installed()).unwrap();
    backend
        .commit(&paths, &plan, GameProcess::NotRunning)
        .unwrap();
    assert_eq!(w.folders(), ["Bar", "Foo"]);
    w.name("Foo", NameChoice::FolderName);
    assert_eq!(w.identity_of("Foo"), "Foo");
}
