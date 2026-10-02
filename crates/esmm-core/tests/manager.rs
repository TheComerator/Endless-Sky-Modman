//! Planning and commit orchestration (decision C), end to end but fully offline: a `Fetcher`
//! serves plugin zips built on the fly in a temp dir instead of hitting the network.

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
use esmm_core::plugin_state::PluginStates;
use esmm_core::records::{self, InstallRecords};
use esmm_core::resolve::Issue;
use zip::write::SimpleFileOptions;

// ---------------------------------------------------------------------------
// Fixture building: plugin zips with real DataNode `plugin.txt` syntax.
// ---------------------------------------------------------------------------

fn plugin_txt(
    name: Option<&str>,
    requires: &[&str],
    optional: &[&str],
    conflicts: &[&str],
    game_version: Option<&str>,
) -> String {
    let mut out = String::new();
    if let Some(name) = name {
        out += &format!("name \"{name}\"\n");
    }
    out += "version 1.0\n";
    let mut deps = String::new();
    if let Some(v) = game_version {
        deps += &format!("\t\"game version\" {v}\n");
    }
    for (label, names) in [
        ("requires", requires),
        ("optional", optional),
        ("conflicts", conflicts),
    ] {
        if !names.is_empty() {
            deps += &format!("\t{label}\n");
            for n in names {
                deps += &format!("\t\t\"{n}\"\n");
            }
        }
    }
    if !deps.is_empty() {
        out += "dependencies\n";
        out += &deps;
    }
    out
}

fn make_plugin_zip(path: &Path, wrapper: &str, plugin_txt_content: &str) {
    let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
    let opts = SimpleFileOptions::default();
    zip.add_directory(format!("{wrapper}/data/"), opts).unwrap();
    zip.start_file(format!("{wrapper}/data/x.txt"), opts)
        .unwrap();
    zip.write_all(b"ship stuff").unwrap();
    zip.start_file(format!("{wrapper}/plugin.txt"), opts)
        .unwrap();
    zip.write_all(plugin_txt_content.as_bytes()).unwrap();
    zip.finish().unwrap();
}

fn entry(catalog_name: &str, version: &str) -> CatalogEntry {
    CatalogEntry {
        name: catalog_name.to_string(),
        authors: "Test Author".into(),
        homepage: "https://example.com".into(),
        license: "MIT".into(),
        version: version.into(),
        short_description: "".into(),
        description: None,
        url: format!("https://example.com/{catalog_name}.zip"),
        icon_url: None,
        autoupdate: None,
    }
}

/// Serves plugin zips from disk by catalog URL, instead of downloading. `fetch_errors` lets a
/// test simulate a `CatalogDownloadFailed` for a specific URL.
#[derive(Default)]
struct TestFetcher {
    files: HashMap<String, PathBuf>,
}

impl Fetcher for TestFetcher {
    fn fetch(
        &self,
        entry: &CatalogEntry,
        dest: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>),
        _cancel: &AtomicBool,
    ) -> Result<Downloaded, String> {
        let src = self
            .files
            .get(&entry.url)
            .ok_or_else(|| format!("no fixture registered for {}", entry.url))?;
        let bytes = fs::read(src).map_err(|e| e.to_string())?;
        progress(0, Some(bytes.len() as u64));
        fs::write(dest, &bytes).map_err(|e| e.to_string())?;
        progress(bytes.len() as u64, Some(bytes.len() as u64));
        Ok(Downloaded {
            bytes: bytes.len() as u64,
            sha256: "test-sha".into(),
        })
    }
}

/// A fake game config dir plus a catalog of fixture plugins ready to "download".
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

    /// Registers a catalog entry whose download is a zip wrapping a single plugin folder
    /// named `catalog_name`-src, with the given `plugin.txt` fields (`name` is the plugin's
    /// own identity; when `None` the folder name becomes the identity).
    #[allow(clippy::too_many_arguments)]
    fn add_plugin(
        &mut self,
        catalog_name: &str,
        name: Option<&str>,
        requires: &[&str],
        optional: &[&str],
        conflicts: &[&str],
        game_version: Option<&str>,
    ) -> CatalogEntry {
        let e = entry(catalog_name, "1.0");
        let zip_path = self.dir.path().join(format!("{catalog_name}.zip"));
        let txt = plugin_txt(name, requires, optional, conflicts, game_version);
        make_plugin_zip(&zip_path, &format!("{catalog_name}-src"), &txt);
        self.fetcher.files.insert(e.url.clone(), zip_path);
        self.catalog.push(e.clone());
        e
    }

    fn plugins_dir(&self) -> PathBuf {
        install::plugins_dir(&self.config)
    }

    fn tmp_dir(&self) -> PathBuf {
        install::tmp_dir(&self.config)
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

    fn commit_ctx(&self) -> CommitContext<'_> {
        CommitContext::new(&self.config, &self.records_path, &self.profiles_path)
    }

    fn folder_names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.plugins_dir())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

/// A `PlanContext` over a snapshot of `world`'s current on-disk state.
struct Snapshot {
    installed: Vec<InstalledPlugin>,
    records: InstallRecords,
    states: PluginStates,
}

impl Snapshot {
    fn take(world: &World) -> Self {
        Snapshot {
            installed: world.installed(),
            records: world.records(),
            states: world.states(),
        }
    }

    fn ctx<'a>(&'a self, world: &'a World, game_version: Option<&'a str>) -> PlanContext<'a> {
        PlanContext {
            catalog: &world.catalog,
            installed: &self.installed,
            records: &self.records,
            states: &self.states,
            game_version,
            config_dir: &world.config,
            fetcher: &world.fetcher,
        }
    }
}

fn always_running() -> GameProcess {
    GameProcess::Running
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn simple_install() {
    let mut world = World::new();
    let a = world.add_plugin("A-Cat", Some("Plugin A"), &[], &[], &[], None);

    let snap = Snapshot::take(&world);
    let plan = manager::plan_install(&a, &snap.ctx(&world, None));
    assert!(plan.issues.is_empty());
    assert_eq!(plan.steps.len(), 1);

    let report = manager::commit(plan, &world.commit_ctx(), false).unwrap();
    assert_eq!(report.installed.len(), 1);
    assert_eq!(report.installed[0].identity(), "Plugin A");
    assert_eq!(world.folder_names(), ["A-Cat"]);
    assert_eq!(world.states().get("Plugin A"), Some(&true));
    let records = world.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records["A-Cat"].identity, "Plugin A");
    assert_eq!(
        records["A-Cat"].sha256, "test-sha",
        "the fetcher's SHA-256 reaches the install record (decision A)"
    );
    assert!(world.folder_names_tmp_is_clean());
}

// Small extension trait kept local to these tests: `.esmm-tmp` should be empty after a
// successful commit (staging dirs are consumed/dropped) as well as after a dropped plan.
trait TmpClean {
    fn folder_names_tmp_is_clean(&self) -> bool;
}

impl TmpClean for World {
    fn folder_names_tmp_is_clean(&self) -> bool {
        match fs::read_dir(self.tmp_dir()) {
            Ok(mut entries) => entries.next().is_none(),
            Err(_) => true,
        }
    }
}

#[test]
fn requires_chain_a_b_c() {
    // Catalog names equal identities here (a plugin's `requires` names identities, so a
    // catalog entry can only resolve one when its name is exactly or normalized-equal to it).
    let mut world = World::new();
    world.add_plugin("C", Some("C"), &[], &[], &[], None);
    world.add_plugin("B", Some("B"), &["C"], &[], &[], None);
    let a = world.add_plugin("A", Some("A"), &["B"], &[], &[], None);

    let snap = Snapshot::take(&world);
    let plan = manager::plan_install(&a, &snap.ctx(&world, None));
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    let identities: Vec<&str> = plan
        .steps
        .iter()
        .map(|s| match s {
            manager::PlanStep::Install(staged) => staged.identity.as_str(),
            _ => panic!("unexpected step"),
        })
        .collect();
    assert_eq!(
        identities,
        ["C", "B", "A"],
        "dependencies land before dependents"
    );

    let report = manager::commit(plan, &world.commit_ctx(), false).unwrap();
    assert_eq!(report.installed.len(), 3);
    assert_eq!(world.folder_names(), ["A", "B", "C"]);
}

#[test]
fn diamond_dependency_staged_once() {
    let mut world = World::new();
    world.add_plugin("D", Some("D"), &[], &[], &[], None);
    world.add_plugin("B", Some("B"), &["D"], &[], &[], None);
    world.add_plugin("C", Some("C"), &["D"], &[], &[], None);
    let a = world.add_plugin("A", Some("A"), &["B", "C"], &[], &[], None);

    let snap = Snapshot::take(&world);
    let plan = manager::plan_install(&a, &snap.ctx(&world, None));
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    assert_eq!(plan.steps.len(), 4, "D is staged exactly once");

    let report = manager::commit(plan, &world.commit_ctx(), false).unwrap();
    assert_eq!(report.installed.len(), 4);
}

#[test]
fn cycle_terminates() {
    let mut world = World::new();
    world.add_plugin("B", Some("B"), &["A"], &[], &[], None);
    let a = world.add_plugin("A", Some("A"), &["B"], &[], &[], None);

    let snap = Snapshot::take(&world);
    let plan = manager::plan_install(&a, &snap.ctx(&world, None));
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    assert_eq!(
        plan.steps.len(),
        2,
        "A and B each staged once, no infinite loop"
    );

    manager::commit(plan, &world.commit_ctx(), false).unwrap();
    assert_eq!(world.folder_names(), ["A", "B"]);
}

#[test]
fn requirement_installed_but_disabled_gets_enabled() {
    let mut world = World::new();
    let b = world.add_plugin("B", Some("B"), &[], &[], &[], None);
    let a = world.add_plugin("A", Some("A"), &["B"], &[], &[], None);

    // Install B first, then disable it.
    {
        let snap = Snapshot::take(&world);
        let plan = manager::plan_install(&b, &snap.ctx(&world, None));
        manager::commit(plan, &world.commit_ctx(), false).unwrap();
    }
    let mut states = world.states();
    states.insert("B".to_string(), false);
    game_state::write_plugin_states(&world.config, &states, GameProcess::NotRunning).unwrap();
    assert_eq!(world.states().get("B"), Some(&false));

    let snap = Snapshot::take(&world);
    let plan = manager::plan_install(&a, &snap.ctx(&world, None));
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    assert!(
        plan.steps
            .iter()
            .any(|s| matches!(s, manager::PlanStep::Enable(id) if id == "B"))
    );

    manager::commit(plan, &world.commit_ctx(), false).unwrap();
    assert_eq!(
        world.states().get("B"),
        Some(&true),
        "B is re-enabled, not reinstalled"
    );
    assert_eq!(world.folder_names(), ["A", "B"]);
}

#[test]
fn requirement_not_in_catalog_blocks_then_override_proceeds() {
    let mut world = World::new();
    let a = world.add_plugin("A-Cat", Some("A"), &["Nonexistent"], &[], &[], None);

    let snap = Snapshot::take(&world);
    let plan = manager::plan_install(&a, &snap.ctx(&world, None));
    assert_eq!(
        plan.issues,
        [Issue::MissingRequirement {
            plugin: "A".into(),
            requires: "Nonexistent".into()
        }]
    );

    // Re-plan (the previous plan's staging would otherwise be consumed by a failed commit
    // attempt) and commit with the override.
    let snap2 = Snapshot::take(&world);
    let plan2 = manager::plan_install(&a, &snap2.ctx(&world, None));
    assert!(matches!(
        manager::commit(plan2, &world.commit_ctx(), false),
        Err(CommitError::Blocked(_))
    ));
    assert!(
        world.folder_names().is_empty(),
        "blocked commit touches nothing"
    );

    let snap3 = Snapshot::take(&world);
    let plan3 = manager::plan_install(&a, &snap3.ctx(&world, None));
    manager::commit(plan3, &world.commit_ctx(), true).unwrap();
    assert_eq!(world.folder_names(), ["A-Cat"]);
}

#[test]
fn installing_a_catalog_match_for_an_existing_unmanaged_identity_is_blocked_then_overridable() {
    let mut world = World::new();
    let new_entry = world.add_plugin("New-Cat", Some("Shared"), &[], &[], &[], None);

    // An unmanaged plugin already on disk under a different folder, same identity -- the
    // "ambiguous, couldn't auto-adopt" scenario this guards against (decision B): the game
    // would only ever load one of the two folders.
    let existing = world.plugins_dir().join("Existing-Unmanaged");
    fs::create_dir_all(existing.join("data")).unwrap();
    fs::write(existing.join("plugin.txt"), "name \"Shared\"\n").unwrap();

    let snap = Snapshot::take(&world);
    let plan = manager::plan_install(&new_entry, &snap.ctx(&world, None));
    assert_eq!(
        plan.issues,
        [Issue::DuplicateIdentity {
            identity: "Shared".into(),
            existing_folder: "Existing-Unmanaged".into(),
            new_folder: "New-Cat".into(),
        }]
    );

    let snap2 = Snapshot::take(&world);
    let plan2 = manager::plan_install(&new_entry, &snap2.ctx(&world, None));
    assert!(matches!(
        manager::commit(plan2, &world.commit_ctx(), false),
        Err(CommitError::Blocked(_))
    ));
    assert_eq!(
        world.folder_names(),
        ["Existing-Unmanaged"],
        "blocked commit touches nothing"
    );

    // Overriding still proceeds (the general escape hatch, decision C) -- it just can no
    // longer happen silently.
    let snap3 = Snapshot::take(&world);
    let plan3 = manager::plan_install(&new_entry, &snap3.ctx(&world, None));
    manager::commit(plan3, &world.commit_ctx(), true).unwrap();
    assert_eq!(world.folder_names(), ["Existing-Unmanaged", "New-Cat"]);
}

#[test]
fn ambiguous_normalized_match_blocks() {
    let mut world = World::new();
    // Both catalog names normalize to "dup" without exactly equaling the required "Dup".
    world.add_plugin("Dup!", Some("Something1"), &[], &[], &[], None);
    world.add_plugin("DUP", Some("Something2"), &[], &[], &[], None);
    let a = world.add_plugin("A", Some("A"), &["Dup"], &[], &[], None);

    let snap = Snapshot::take(&world);
    let plan = manager::plan_install(&a, &snap.ctx(&world, None));
    match &plan.issues[..] {
        [
            Issue::AmbiguousRequirement {
                plugin,
                requires,
                candidates,
            },
        ] => {
            assert_eq!(plugin, "A");
            assert_eq!(requires, "Dup");
            assert_eq!(candidates.len(), 2);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn likely_match_identity_mismatch_still_installs_under_its_real_identity() {
    let mut world = World::new();
    // Catalog name "Jimmys-Ship-Emporium" normalizes the same as the required identity
    // "Jimmy's Ship Emporium", but the actual plugin.txt name differs from what was required.
    world.add_plugin(
        "Jimmys-Ship-Emporium",
        Some("Actually Different Name"),
        &[],
        &[],
        &[],
        None,
    );
    let a = world.add_plugin(
        "A-Cat",
        Some("A"),
        &["Jimmy's Ship Emporium"],
        &[],
        &[],
        None,
    );

    let snap = Snapshot::take(&world);
    let plan = manager::plan_install(&a, &snap.ctx(&world, None));
    assert_eq!(
        plan.issues,
        [Issue::IdentityMismatch {
            expected: "Jimmy's Ship Emporium".into(),
            catalog_name: "Jimmys-Ship-Emporium".into(),
            actual: "Actually Different Name".into(),
        }]
    );
    // It's still staged (just doesn't satisfy the requirement under the expected name).
    assert_eq!(plan.steps.len(), 2);
}

#[test]
fn conflict_declared_from_either_side() {
    // Existing, enabled plugin declares the conflict; new plugin doesn't.
    let mut world = World::new();
    let existing = world.add_plugin("Old-Cat", Some("Old"), &[], &[], &["New"], None);
    {
        let snap = Snapshot::take(&world);
        let plan = manager::plan_install(&existing, &snap.ctx(&world, None));
        manager::commit(plan, &world.commit_ctx(), false).unwrap();
    }
    let new = world.add_plugin("New-Cat", Some("New"), &[], &[], &[], None);
    let snap = Snapshot::take(&world);
    let plan = manager::plan_install(&new, &snap.ctx(&world, None));
    assert_eq!(
        plan.issues,
        [Issue::Conflict {
            a: "Old".into(),
            b: "New".into()
        }]
    );

    // New plugin declares the conflict; existing one doesn't. Different pair, same shape.
    let mut world2 = World::new();
    let existing2 = world2.add_plugin("Old2-Cat", Some("Old2"), &[], &[], &[], None);
    {
        let snap = Snapshot::take(&world2);
        let plan = manager::plan_install(&existing2, &snap.ctx(&world2, None));
        manager::commit(plan, &world2.commit_ctx(), false).unwrap();
    }
    let new2 = world2.add_plugin("New2-Cat", Some("New2"), &[], &[], &["Old2"], None);
    let snap2 = Snapshot::take(&world2);
    let plan2 = manager::plan_install(&new2, &snap2.ctx(&world2, None));
    assert_eq!(
        plan2.issues,
        [Issue::Conflict {
            a: "Old2".into(),
            b: "New2".into()
        }]
    );
}

#[test]
fn conflict_resolved_via_disable_to_resolve() {
    let mut world = World::new();
    let existing = world.add_plugin("Old-Cat", Some("Old"), &[], &[], &["New"], None);
    {
        let snap = Snapshot::take(&world);
        let plan = manager::plan_install(&existing, &snap.ctx(&world, None));
        manager::commit(plan, &world.commit_ctx(), false).unwrap();
    }
    let new = world.add_plugin("New-Cat", Some("New"), &[], &[], &[], None);
    let snap = Snapshot::take(&world);
    let mut plan = manager::plan_install(&new, &snap.ctx(&world, None));
    assert_eq!(plan.issues.len(), 1);
    plan.disable_to_resolve("Old");
    assert!(plan.issues.is_empty());

    let report = manager::commit(plan, &world.commit_ctx(), false).unwrap();
    assert_eq!(report.disabled, ["Old".to_string()]);
    assert_eq!(world.states().get("Old"), Some(&false));
    assert_eq!(world.states().get("New"), Some(&true));
}

#[test]
fn enable_conflict_resolved_via_disable_to_resolve() {
    let mut world = World::new();
    let new = world.add_plugin("New-Cat", Some("New"), &[], &[], &["Old"], None);
    let old = world.add_plugin("Old-Cat", Some("Old"), &[], &[], &[], None);

    // Install New first (Old doesn't exist yet, so its declared conflict can't trip), then
    // disable it, then install Old -- so both exist but only Old is enabled, and the conflict
    // only surfaces when New is re-enabled.
    {
        let snap = Snapshot::take(&world);
        manager::commit(
            manager::plan_install(&new, &snap.ctx(&world, None)),
            &world.commit_ctx(),
            false,
        )
        .unwrap();
    }
    let mut states = world.states();
    states.insert("New".to_string(), false);
    game_state::write_plugin_states(&world.config, &states, GameProcess::NotRunning).unwrap();
    {
        let snap = Snapshot::take(&world);
        manager::commit(
            manager::plan_install(&old, &snap.ctx(&world, None)),
            &world.commit_ctx(),
            false,
        )
        .unwrap();
    }

    let snap = Snapshot::take(&world);
    let mut plan = manager::plan_enable("New", &snap.ctx(&world, None));
    assert_eq!(plan.issues.len(), 1, "{:?}", plan.issues);
    plan.disable_to_resolve("Old");
    assert!(plan.issues.is_empty());

    let report = manager::commit_enable(plan, &world.commit_ctx(), false).unwrap();
    assert_eq!(report.enabled, ["New".to_string()]);
    assert_eq!(report.disabled, ["Old".to_string()]);
    assert_eq!(world.states().get("Old"), Some(&false));
    assert_eq!(world.states().get("New"), Some(&true));
}

#[test]
fn conflict_between_two_plugins_inside_one_plan() {
    let mut world = World::new();
    world.add_plugin("X", Some("X"), &[], &[], &["Y"], None);
    world.add_plugin("Y", Some("Y"), &[], &[], &[], None);
    let root = world.add_plugin("Root", Some("Root"), &["X", "Y"], &[], &[], None);

    let snap = Snapshot::take(&world);
    let plan = manager::plan_install(&root, &snap.ctx(&world, None));
    assert_eq!(
        plan.issues,
        [Issue::Conflict {
            a: "X".into(),
            b: "Y".into()
        }]
    );
}

#[test]
fn game_version_too_old_unknown_and_satisfied() {
    let mut world = World::new();
    let a = world.add_plugin("A-Cat", Some("A"), &[], &[], &[], Some("0.10.13.1"));

    let snap = Snapshot::take(&world);
    let plan = manager::plan_install(&a, &snap.ctx(&world, Some("0.10.0")));
    assert_eq!(
        plan.issues,
        [Issue::GameTooOld {
            plugin: "A".into(),
            required: "0.10.13.1".into(),
            actual: "0.10.0".into(),
        }]
    );

    let plan = manager::plan_install(&a, &snap.ctx(&world, None));
    assert_eq!(
        plan.issues,
        [Issue::GameVersionUnknown {
            plugin: "A".into(),
            required: "0.10.13.1".into()
        }]
    );

    let plan = manager::plan_install(&a, &snap.ctx(&world, Some("0.11.3.0")));
    assert!(plan.issues.is_empty());
}

#[test]
fn commit_refuses_while_game_running_and_touches_nothing() {
    let mut world = World::new();
    let a = world.add_plugin("A-Cat", Some("A"), &[], &[], &[], None);
    let snap = Snapshot::take(&world);
    let plan = manager::plan_install(&a, &snap.ctx(&world, None));

    let mut ctx = world.commit_ctx();
    ctx.detect_game = always_running;
    assert!(matches!(
        manager::commit(plan, &ctx, false),
        Err(CommitError::GameRunning)
    ));
    assert!(world.folder_names().is_empty());
    assert!(!world.config.join("plugins.txt").exists());
    assert!(!world.records_path.exists());
}

#[test]
fn uninstall_and_disable_blocked_by_enabled_dependent() {
    let mut world = World::new();
    world.add_plugin("B", Some("B"), &[], &[], &[], None);
    let a = world.add_plugin("A", Some("A"), &["B"], &[], &[], None);
    {
        let snap = Snapshot::take(&world);
        let plan = manager::plan_install(&a, &snap.ctx(&world, None));
        manager::commit(plan, &world.commit_ctx(), false).unwrap();
    }

    let snap = Snapshot::take(&world);
    let ctx = snap.ctx(&world, None);
    let disable_plan = manager::plan_disable("B", &ctx);
    assert_eq!(
        disable_plan.issues,
        [Issue::RequiredBy {
            plugin: "B".into(),
            dependents: vec!["A".into()]
        }]
    );
    assert!(matches!(
        manager::commit_disable(disable_plan, &world.commit_ctx(), false),
        Err(CommitError::Blocked(_))
    ));

    let uninstall_plan = manager::plan_uninstall("B", &ctx);
    assert_eq!(
        uninstall_plan.issues,
        [Issue::RequiredBy {
            plugin: "B".into(),
            dependents: vec!["A".into()]
        }]
    );
    assert!(matches!(
        manager::commit_uninstall(uninstall_plan, &world.commit_ctx(), false),
        Err(CommitError::Blocked(_))
    ));
    // Nothing changed: B is still on disk and still enabled.
    assert_eq!(world.folder_names(), ["A", "B"]);
    assert_eq!(world.states().get("B"), Some(&true));
}

#[test]
fn update_adds_a_new_requirement() {
    let mut world = World::new();
    let a_v1 = world.add_plugin("A-Cat", Some("A"), &[], &[], &[], None);
    {
        let snap = Snapshot::take(&world);
        let plan = manager::plan_install(&a_v1, &snap.ctx(&world, None));
        manager::commit(plan, &world.commit_ctx(), false).unwrap();
    }

    // The catalog's current version of A now requires "New", which isn't installed.
    world.catalog.retain(|e| e.name != "A-Cat");
    let a_v2 = entry("A-Cat", "2.0");
    let zip_path = world.dir.path().join("A-Cat-v2.zip");
    make_plugin_zip(
        &zip_path,
        "A-Cat-src",
        &plugin_txt(Some("A"), &["New"], &[], &[], None),
    );
    world.fetcher.files.insert(a_v2.url.clone(), zip_path);
    world.catalog.push(a_v2);

    let snap = Snapshot::take(&world);
    let ctx = snap.ctx(&world, None);
    let update_plan = manager::plan_update("A-Cat", &ctx).unwrap();
    assert_eq!(
        update_plan.issues,
        [Issue::MissingRequirement {
            plugin: "A".into(),
            requires: "New".into()
        }]
    );
    manager::commit_update(update_plan, &world.commit_ctx(), true).unwrap();
    assert_eq!(world.records()["A-Cat"].version, "2.0");
}

#[test]
fn update_keeps_enabled_state_and_folder_name() {
    let mut world = World::new();
    let a_v1 = world.add_plugin("A-Cat", Some("A"), &[], &[], &[], None);
    {
        let snap = Snapshot::take(&world);
        let plan = manager::plan_install(&a_v1, &snap.ctx(&world, None));
        manager::commit(plan, &world.commit_ctx(), false).unwrap();
    }
    let mut states = world.states();
    states.insert("A".to_string(), false);
    game_state::write_plugin_states(&world.config, &states, GameProcess::NotRunning).unwrap();

    world.catalog.retain(|e| e.name != "A-Cat");
    let a_v2 = entry("A-Cat", "2.0");
    let zip_path = world.dir.path().join("A-Cat-v2.zip");
    make_plugin_zip(
        &zip_path,
        "A-Cat-src-v2",
        &plugin_txt(Some("A"), &[], &[], &[], None),
    );
    world.fetcher.files.insert(a_v2.url.clone(), zip_path);
    world.catalog.push(a_v2);

    let snap = Snapshot::take(&world);
    let ctx = snap.ctx(&world, None);
    let update_plan = manager::plan_update("A-Cat", &ctx).unwrap();
    assert!(update_plan.issues.is_empty());
    manager::commit_update(update_plan, &world.commit_ctx(), false).unwrap();

    assert_eq!(
        world.folder_names(),
        ["A-Cat"],
        "folder name is stable across updates"
    );
    assert_eq!(
        world.states().get("A"),
        Some(&false),
        "stays disabled across the update"
    );
    assert_eq!(world.records()["A-Cat"].version, "2.0");
}

#[test]
fn update_all_commits_every_staged_plugin() {
    let mut world = World::new();
    let a_v1 = world.add_plugin("A-Cat", Some("A"), &[], &[], &[], None);
    let b_v1 = world.add_plugin("B-Cat", Some("B"), &[], &[], &[], None);
    {
        let snap = Snapshot::take(&world);
        let ctx = snap.ctx(&world, None);
        manager::commit(
            manager::plan_install(&a_v1, &ctx),
            &world.commit_ctx(),
            false,
        )
        .unwrap();
    }
    {
        let snap = Snapshot::take(&world);
        let ctx = snap.ctx(&world, None);
        manager::commit(
            manager::plan_install(&b_v1, &ctx),
            &world.commit_ctx(),
            false,
        )
        .unwrap();
    }

    for (cat, src) in [("A-Cat", "A"), ("B-Cat", "B")] {
        world.catalog.retain(|e| e.name != cat);
        let v2 = entry(cat, "2.0");
        let zip_path = world.dir.path().join(format!("{cat}-v2.zip"));
        make_plugin_zip(
            &zip_path,
            &format!("{cat}-src-v2"),
            &plugin_txt(Some(src), &[], &[], &[], None),
        );
        world.fetcher.files.insert(v2.url.clone(), zip_path);
        world.catalog.push(v2);
    }

    let snap = Snapshot::take(&world);
    let ctx = snap.ctx(&world, None);
    let plans = vec![
        manager::plan_update("A-Cat", &ctx).unwrap(),
        manager::plan_update("B-Cat", &ctx).unwrap(),
    ];
    assert!(plans.iter().all(|p| p.issues.is_empty()));
    let report = manager::commit_update_all(plans, &world.commit_ctx(), false).unwrap();
    assert_eq!(report.installed.len(), 2);

    assert_eq!(world.records()["A-Cat"].version, "2.0");
    assert_eq!(world.records()["B-Cat"].version, "2.0");
    assert_eq!(
        world.folder_names(),
        ["A-Cat", "B-Cat"],
        "folder names are stable across the batch"
    );
}

#[test]
fn update_all_blocked_issue_refuses_the_whole_batch_until_overridden() {
    let mut world = World::new();
    let a_v1 = world.add_plugin("A-Cat", Some("A"), &[], &[], &[], None);
    let b_v1 = world.add_plugin("B-Cat", Some("B"), &[], &[], &[], None);
    {
        let snap = Snapshot::take(&world);
        let ctx = snap.ctx(&world, None);
        manager::commit(
            manager::plan_install(&a_v1, &ctx),
            &world.commit_ctx(),
            false,
        )
        .unwrap();
        manager::commit(
            manager::plan_install(&b_v1, &ctx),
            &world.commit_ctx(),
            false,
        )
        .unwrap();
    }

    // B-Cat's new version adds a requirement that isn't installed: that plan alone is
    // blocked, which must block committing the batch at all without an override.
    world.catalog.retain(|e| e.name != "B-Cat");
    let b_v2 = entry("B-Cat", "2.0");
    let zip_path = world.dir.path().join("B-Cat-v2.zip");
    make_plugin_zip(
        &zip_path,
        "B-Cat-src-v2",
        &plugin_txt(Some("B"), &["New"], &[], &[], None),
    );
    world.fetcher.files.insert(b_v2.url.clone(), zip_path);
    world.catalog.push(b_v2);
    world.catalog.retain(|e| e.name != "A-Cat");
    let a_v2 = entry("A-Cat", "2.0");
    let zip_path = world.dir.path().join("A-Cat-v2.zip");
    make_plugin_zip(
        &zip_path,
        "A-Cat-src-v2",
        &plugin_txt(Some("A"), &[], &[], &[], None),
    );
    world.fetcher.files.insert(a_v2.url.clone(), zip_path);
    world.catalog.push(a_v2);

    let snap = Snapshot::take(&world);
    let ctx = snap.ctx(&world, None);
    let plans = vec![
        manager::plan_update("A-Cat", &ctx).unwrap(),
        manager::plan_update("B-Cat", &ctx).unwrap(),
    ];
    let err = manager::commit_update_all(plans, &world.commit_ctx(), false).unwrap_err();
    assert!(matches!(err, CommitError::Blocked(_)));
    // Nothing committed: refusing is checked before anything is installed.
    assert_eq!(world.records()["A-Cat"].version, "1.0");
    assert_eq!(world.records()["B-Cat"].version, "1.0");

    let snap = Snapshot::take(&world);
    let ctx = snap.ctx(&world, None);
    let plans = vec![
        manager::plan_update("A-Cat", &ctx).unwrap(),
        manager::plan_update("B-Cat", &ctx).unwrap(),
    ];
    manager::commit_update_all(plans, &world.commit_ctx(), true).unwrap();
    assert_eq!(world.records()["A-Cat"].version, "2.0");
    assert_eq!(world.records()["B-Cat"].version, "2.0");
}

#[cfg(unix)]
#[test]
fn update_all_mid_batch_failure_persists_what_succeeded() {
    use std::os::unix::fs::PermissionsExt;

    let mut world = World::new();
    let a_v1 = world.add_plugin("A-Cat", Some("A"), &[], &[], &[], None);
    let b_v1 = world.add_plugin("B-Cat", Some("B"), &[], &[], &[], None);
    {
        let snap = Snapshot::take(&world);
        let ctx = snap.ctx(&world, None);
        manager::commit(
            manager::plan_install(&a_v1, &ctx),
            &world.commit_ctx(),
            false,
        )
        .unwrap();
        manager::commit(
            manager::plan_install(&b_v1, &ctx),
            &world.commit_ctx(),
            false,
        )
        .unwrap();
    }

    for (cat, src) in [("A-Cat", "A"), ("B-Cat", "B")] {
        world.catalog.retain(|e| e.name != cat);
        let v2 = entry(cat, "2.0");
        let zip_path = world.dir.path().join(format!("{cat}-v2.zip"));
        make_plugin_zip(
            &zip_path,
            &format!("{cat}-src-v2"),
            &plugin_txt(Some(src), &[], &[], &[], None),
        );
        world.fetcher.files.insert(v2.url.clone(), zip_path);
        world.catalog.push(v2);
    }

    let snap = Snapshot::take(&world);
    let ctx = snap.ctx(&world, None);
    let plans = vec![
        manager::plan_update("A-Cat", &ctx).unwrap(),
        manager::plan_update("B-Cat", &ctx).unwrap(),
    ];

    // Sabotage only B's staged copy (second in the batch), same trick as
    // `mid_commit_failure_leaves_records_and_plugins_txt_consistent`: A must already be
    // persisted by the time B's install fails.
    let staged_b = find_dir_named(&world.tmp_dir(), "B-Cat-src-v2").expect("B's staged folder");
    let locked_parent = staged_b.parent().unwrap().to_path_buf();
    fs::set_permissions(&locked_parent, fs::Permissions::from_mode(0o555)).unwrap();
    if fs::write(locked_parent.join("probe"), "").is_ok() {
        fs::set_permissions(&locked_parent, fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("skipping: permissions not enforced (running as root?)");
        return;
    }

    let result = manager::commit_update_all(plans, &world.commit_ctx(), false);
    fs::set_permissions(&locked_parent, fs::Permissions::from_mode(0o755)).unwrap();

    let (report, folder) = match result {
        Err(CommitError::InstallFailed { report, folder, .. }) => (report, folder),
        other => panic!("{other:?}"),
    };
    assert_eq!(folder, "B-Cat");
    assert_eq!(report.installed.len(), 1);
    assert_eq!(report.installed[0].identity(), "A");
    assert_eq!(
        world.records()["A-Cat"].version,
        "2.0",
        "A's update was persisted before B's failed"
    );
    assert_eq!(
        world.records()["B-Cat"].version,
        "1.0",
        "B's update never committed"
    );
}

#[test]
fn dropping_an_uncommitted_plan_cleans_up_staging() {
    let mut world = World::new();
    world.add_plugin("C", Some("C"), &[], &[], &[], None);
    let a = world.add_plugin("A", Some("A"), &["C"], &[], &[], None);

    {
        let snap = Snapshot::take(&world);
        let plan = manager::plan_install(&a, &snap.ctx(&world, None));
        assert_eq!(plan.steps.len(), 2);
        // Plan is dropped here without ever calling commit.
    }
    assert!(world.folder_names_tmp_is_clean());
    assert!(
        world.folder_names().is_empty(),
        "plugins/ was never touched"
    );
}

#[test]
fn adoption_exact_likely_and_ambiguous() {
    let mut world = World::new();
    world.add_plugin("Exact-Cat", Some("Exact-Cat"), &[], &[], &[], None); // name == catalog name: Exact
    world.add_plugin(
        "Jimmys-Ship-Emporium",
        Some("Jimmy's Ship Emporium"),
        &[],
        &[],
        &[],
        None,
    ); // Likely
    // Both normalize to "dup" without exactly equaling the unmanaged plugin's identity "Dup" below.
    world.add_plugin("Dup!", Some("Something1"), &[], &[], &[], None);
    world.add_plugin("DUP", Some("Something2"), &[], &[], &[], None);

    // Install all four "by hand" (bypassing plan/commit, simulating an unmanaged drop-in):
    // extract each fixture straight into plugins/ with no install record.
    for (folder, identity) in [
        ("Exact-Cat", "Exact-Cat"),
        ("Jimmys-Ship-Emporium", "Jimmy's Ship Emporium"),
        ("Dup-Folder-1", "Dup"),
        ("Dup-Folder-2", "Dup"),
    ] {
        let dest = world.plugins_dir().join(folder);
        fs::create_dir_all(dest.join("data")).unwrap();
        fs::write(dest.join("plugin.txt"), format!("name \"{identity}\"\n")).unwrap();
    }

    let installed = world.installed();
    assert_eq!(installed.len(), 4);
    assert!(installed.iter().all(|p| p.record.is_none()));

    let result = manager::adopt_candidates(&installed, &world.catalog, &world.records());
    assert_eq!(result.adopted.len(), 2, "Exact and Likely auto-adopt");
    assert!(
        result
            .adopted
            .iter()
            .any(|r| r.folder == "Exact-Cat" && r.version.is_empty())
    );
    assert!(
        result
            .adopted
            .iter()
            .any(|r| r.folder == "Jimmys-Ship-Emporium")
    );
    assert_eq!(
        result.needs_user.len(),
        2,
        "the ambiguous pair needs the user"
    );
    for u in &result.needs_user {
        assert_eq!(u.candidates.len(), 2);
    }
}

/// Recursively finds a directory literally named `name` under `root` (depth-first). Used to
/// locate a specific staged plugin's extracted folder on disk without needing access to
/// `StagedPlugin`'s private `root` field.
#[cfg(unix)]
fn find_dir_named(root: &Path, name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() == name {
                return Some(path);
            }
            if let Some(found) = find_dir_named(&path, name) {
                return Some(found);
            }
        }
    }
    None
}

#[cfg(unix)]
#[test]
fn mid_commit_failure_leaves_records_and_plugins_txt_consistent() {
    use std::os::unix::fs::PermissionsExt;

    let mut world = World::new();
    world.add_plugin("B", Some("B"), &[], &[], &[], None);
    let a = world.add_plugin("A", Some("A"), &["B"], &[], &[], None);

    let snap = Snapshot::take(&world);
    let plan = manager::plan_install(&a, &snap.ctx(&world, None));
    assert_eq!(
        plan.steps.len(),
        2,
        "B is staged, then A, which requires it"
    );

    // Sabotage only A's staged copy: a read-only parent makes its folder un-renameable (the
    // same trick as `install::tests::failed_restore_keeps_the_old_copy_and_reports_it`), while
    // B's separate staging directory is untouched and installs normally.
    let staged_a = find_dir_named(&world.tmp_dir(), "A-src").expect("A's staged folder");
    let locked_parent = staged_a.parent().unwrap().to_path_buf();
    fs::set_permissions(&locked_parent, fs::Permissions::from_mode(0o555)).unwrap();
    if fs::write(locked_parent.join("probe"), "").is_ok() {
        fs::set_permissions(&locked_parent, fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("skipping: permissions not enforced (running as root?)");
        return;
    }

    let result = manager::commit(plan, &world.commit_ctx(), false);
    fs::set_permissions(&locked_parent, fs::Permissions::from_mode(0o755)).unwrap();

    let (report, folder) = match result {
        Err(CommitError::InstallFailed { report, folder, .. }) => (report, folder),
        other => panic!("{other:?}"),
    };
    assert_eq!(folder, "A");
    assert_eq!(report.installed.len(), 1);
    assert_eq!(report.installed[0].identity(), "B");

    // On-disk state matches exactly what succeeded: B is recorded and enabled, A is not.
    let records = world.records();
    assert_eq!(records.len(), 1);
    assert!(records.contains_key("B"));
    let states = world.states();
    assert_eq!(states.get("B"), Some(&true));
    assert!(!states.contains_key("A"));
    assert_eq!(world.folder_names(), ["B"]);
}
