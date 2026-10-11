//! Load-order management together with the rest of the manager: installs, updates, uninstalls,
//! `plugins.txt` and profiles must keep working on folders that carry a number prefix. All
//! offline, in temp dirs; a `Fetcher` serves zips built on the fly.
//!
//! This file belongs to the optional load-order feature (see `src/load_order.rs`).

use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use esmm_core::catalog::CatalogEntry;
use esmm_core::download::Downloaded;
use esmm_core::game_state::{self, GameProcess};
use esmm_core::install;
use esmm_core::load_order::{self, LoadOrderBackend, Paths};
use esmm_core::manager::{self, CommitContext, Fetcher, InstalledPlugin, PlanContext};
use esmm_core::plugin_state::PluginStates;
use esmm_core::profiles::ProfileStore;
use esmm_core::records::{self, InstallRecord, InstallRecords};
use zip::write::SimpleFileOptions;

fn make_plugin_zip(path: &Path, wrapper: &str, name: &str) {
    let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
    let opts = SimpleFileOptions::default();
    zip.add_directory(format!("{wrapper}/data/"), opts).unwrap();
    zip.start_file(format!("{wrapper}/data/x.txt"), opts)
        .unwrap();
    zip.write_all(b"ship stuff").unwrap();
    zip.start_file(format!("{wrapper}/plugin.txt"), opts)
        .unwrap();
    zip.write_all(format!("name \"{name}\"\nversion 1.0\n").as_bytes())
        .unwrap();
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

    /// Registers a catalog entry whose zip holds a plugin named `identity`.
    fn catalog_plugin(&mut self, catalog_name: &str, identity: &str, version: &str) {
        let e = entry(catalog_name, version);
        let zip = self
            .dir
            .path()
            .join(format!("{catalog_name}-{version}.zip"));
        make_plugin_zip(&zip, &format!("{catalog_name}-src"), identity);
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

    fn folders(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.plugins_dir())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort_by(|a, b| load_order::game_order(a, b));
        names
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

    fn paths(&self) -> Paths<'_> {
        Paths::new(&self.config, &self.records_path)
    }

    fn apply(&self, wanted: &[&str]) {
        let backend = load_order::default_backend();
        let wanted: Vec<String> = wanted.iter().map(|s| s.to_string()).collect();
        let plan = backend
            .plan_apply(&self.paths(), &self.installed(), &wanted)
            .unwrap();
        backend
            .commit(&self.paths(), &plan, GameProcess::NotRunning)
            .unwrap();
    }

    fn identities(&self) -> BTreeSet<String> {
        self.installed()
            .iter()
            .map(|p| p.identity.clone())
            .collect()
    }
}

/// Two managed plugins, installed and then given an order: B first, A second.
fn ordered_world() -> World {
    let mut world = World::new();
    world.catalog_plugin("A-Cat", "Plugin A", "1.0");
    world.catalog_plugin("B-Cat", "Plugin B", "1.0");
    world.install("A-Cat");
    world.install("B-Cat");
    world.apply(&["B-Cat", "A-Cat"]);
    assert_eq!(world.folders(), ["010-B-Cat", "020-A-Cat"]);
    world
}

#[test]
fn an_update_replaces_the_prefixed_folder_and_keeps_its_prefix_and_original_name() {
    let mut world = ordered_world();
    world.catalog_plugin("A-Cat", "Plugin A", "2.0");

    let plan = world
        .with_ctx(|ctx| manager::plan_update("020-A-Cat", ctx))
        .unwrap();
    assert!(plan.issues.is_empty());
    manager::commit_update(plan, &world.commit_ctx(), false).unwrap();

    assert_eq!(
        world.folders(),
        ["010-B-Cat", "020-A-Cat"],
        "no second copy appears under the catalog name"
    );
    let records = world.records();
    let record = &records["020-A-Cat"];
    assert_eq!(record.version, "2.0");
    assert_eq!(record.sha256, "test-sha");
    assert_eq!(record.original_folder.as_deref(), Some("A-Cat"));
    assert!(!records.contains_key("A-Cat"));
}

#[test]
fn update_all_keeps_every_prefix_and_original_name() {
    let mut world = ordered_world();
    world.catalog_plugin("A-Cat", "Plugin A", "2.0");
    world.catalog_plugin("B-Cat", "Plugin B", "2.0");

    let plans = world.with_ctx(|ctx| {
        vec![
            manager::plan_update("010-B-Cat", ctx).unwrap(),
            manager::plan_update("020-A-Cat", ctx).unwrap(),
        ]
    });
    manager::commit_update_all(plans, &world.commit_ctx(), false).unwrap();

    assert_eq!(world.folders(), ["010-B-Cat", "020-A-Cat"]);
    let records = world.records();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records["010-B-Cat"].original_folder.as_deref(),
        Some("B-Cat")
    );
    assert_eq!(
        records["020-A-Cat"].original_folder.as_deref(),
        Some("A-Cat")
    );
    assert!(records.values().all(|r| r.version == "2.0"));
}

#[test]
fn an_update_of_a_plugin_that_was_never_renamed_still_works_as_before() {
    let mut world = World::new();
    world.catalog_plugin("A-Cat", "Plugin A", "1.0");
    world.install("A-Cat");
    world.catalog_plugin("A-Cat", "Plugin A", "2.0");
    let plan = world
        .with_ctx(|ctx| manager::plan_update("A-Cat", ctx))
        .unwrap();
    manager::commit_update(plan, &world.commit_ctx(), false).unwrap();
    assert_eq!(world.folders(), ["A-Cat"]);
    assert_eq!(world.records()["A-Cat"].original_folder, None);
}

#[test]
fn uninstalling_a_prefixed_plugin_removes_its_folder_and_record() {
    let world = ordered_world();
    let plan = world.with_ctx(|ctx| manager::plan_uninstall("010-B-Cat", ctx));
    assert_eq!(plan.identity, "Plugin B");
    manager::commit_uninstall(plan, &world.commit_ctx(), false).unwrap();

    assert_eq!(world.folders(), ["020-A-Cat"]);
    let records = world.records();
    assert_eq!(records.keys().collect::<Vec<_>>(), ["020-A-Cat"]);
}

#[test]
fn a_plugin_installed_later_is_unnumbered_and_gets_the_next_number_when_placed() {
    let mut world = ordered_world();
    world.catalog_plugin("C-Cat", "Plugin C", "1.0");
    world.install("C-Cat");

    // It sorts after the numbered ones, which is where the layout puts it.
    assert_eq!(world.folders(), ["010-B-Cat", "020-A-Cat", "C-Cat"]);
    let backend = load_order::default_backend();
    let layout = backend.layout(&world.installed());
    let numbers: Vec<Option<u32>> = layout.ordered.iter().map(|p| p.number).collect();
    assert_eq!(numbers, [Some(10), Some(20), None]);

    // Placing it renames only itself.
    let wanted: Vec<String> = layout.ordered.iter().map(|p| p.folder.clone()).collect();
    let plan = backend
        .plan_apply(&world.paths(), &world.installed(), &wanted)
        .unwrap();
    assert_eq!(plan.renames.len(), 1);
    assert_eq!(plan.renames[0].to, "030-C-Cat");
}

#[test]
fn enabling_disabling_and_profiles_still_use_identities_after_a_rename() {
    let world = ordered_world();
    // plugins.txt is keyed by identity; enabling/disabling works on renamed folders.
    assert!(world.installed().iter().all(|p| p.record.is_some()));
    let plan = world.with_ctx(|ctx| manager::plan_disable("Plugin A", ctx));
    assert!(plan.issues.is_empty());
    manager::commit_disable(plan, &world.commit_ctx(), false).unwrap();
    assert_eq!(world.states().get("Plugin A"), Some(&false));
    assert_eq!(
        world.states().keys().collect::<Vec<_>>(),
        ["Plugin A", "Plugin B"]
    );

    // A profile saved now describes the same plugins by identity, with catalog names from the
    // records, whatever the folders are called.
    let snapshot = |w: &World| {
        let mut store = ProfileStore::default();
        store.ensure_default(&w.identities(), &w.states(), &w.records());
        serde_json::to_string(&store.profiles).unwrap()
    };
    let numbered = snapshot(&world);

    let backend = load_order::default_backend();
    let plan = backend
        .plan_restore(&world.paths(), &world.installed())
        .unwrap();
    backend
        .commit(&world.paths(), &plan, GameProcess::NotRunning)
        .unwrap();
    assert_eq!(world.folders(), ["A-Cat", "B-Cat"]);
    assert_eq!(
        snapshot(&world),
        numbered,
        "the profile does not depend on folder names"
    );
    // Only enabled plugins are in a profile: Plugin B, with its catalog name; no folder names.
    assert!(numbered.contains("Plugin B") && numbered.contains("B-Cat"));
    assert!(!numbered.contains("010-") && !numbered.contains("020-"));
}

#[test]
fn renaming_leaves_the_files_inside_every_plugin_untouched() {
    let world = ordered_world();
    for folder in ["010-B-Cat", "020-A-Cat"] {
        let dir = world.plugins_dir().join(folder);
        assert_eq!(
            fs::read_to_string(dir.join("data/x.txt")).unwrap(),
            "ship stuff"
        );
        let mut inside: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        inside.sort();
        assert_eq!(
            inside,
            ["data", "plugin.txt"],
            "nothing was added or removed"
        );
    }
}

#[test]
fn old_record_files_without_the_new_field_still_load() {
    let json = r#"{"Foo": {"catalog_name": "Foo", "folder": "Foo", "identity": "Foo",
        "version": "1", "source_url": "https://example.com/a.zip", "sha256": "ab"}}"#;
    let records: InstallRecords = serde_json::from_str(json).unwrap();
    let record: &InstallRecord = &records["Foo"];
    assert_eq!(record.original_folder, None);
    // And a record that was never renamed is written without the field.
    assert!(
        !serde_json::to_string(record)
            .unwrap()
            .contains("original_folder")
    );
}
