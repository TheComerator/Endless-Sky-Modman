//! Tests for the shell: what's new on top of `esmm-core` (which has its own suite) is the
//! pending-plan lifecycle, install selection and per-install state, the view/error mapping
//! the frontend depends on, and argument marshaling through Tauri's IPC.
//!
//! Fully offline, same approach as `esmm-core/tests/manager.rs`: plugin zips are built on
//! the fly and served by a fake `Fetcher`; the catalog comes from a closure.

use std::cell::Cell;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use esmm_core::catalog::{CatalogEntry, CatalogFetch, FetchSource};
use esmm_core::download::Downloaded;
use esmm_core::game_install::GameInstall;
use esmm_core::game_state::GameProcess;
use esmm_core::install;
use esmm_core::manager::{CommitError, CommitReport, Fetcher};
use esmm_core::profiles::{Profile, ProfileStore};
use esmm_core::records;
use esmm_core::resolve::Issue;
use zip::write::SimpleFileOptions;

use crate::error::CmdError;
use crate::settings::InstallPaths;
use crate::shell::{Deps, Shell};
use crate::views::*;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

thread_local! {
    static GAME_RUNNING: Cell<bool> = const { Cell::new(false) };
}

/// Planning and commit-refusal checks run on the calling (test) thread, so a thread-local
/// lets each test flip "the game is running" without racing other tests.
fn fake_detect_game() -> GameProcess {
    if GAME_RUNNING.with(Cell::get) {
        GameProcess::Running
    } else {
        GameProcess::NotRunning
    }
}

fn fake_version(_: &GameInstall) -> Option<String> {
    Some("0.10.16".into())
}

fn fake_launch(_: &GameInstall) -> std::io::Result<()> {
    Ok(())
}

#[derive(Default)]
struct ZipFetcher {
    files: Mutex<HashMap<String, PathBuf>>,
}

impl Fetcher for ZipFetcher {
    fn fetch(
        &self,
        entry: &CatalogEntry,
        dest: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>),
        _cancel: &AtomicBool,
    ) -> Result<Downloaded, String> {
        let src = self.files.lock().unwrap().get(&entry.url).cloned();
        let src = src.ok_or_else(|| format!("no fixture for {}", entry.url))?;
        let bytes = fs::read(src).map_err(|e| e.to_string())?;
        let total = Some(bytes.len() as u64);
        progress(0, total);
        fs::write(dest, &bytes).map_err(|e| e.to_string())?;
        progress(bytes.len() as u64, total);
        Ok(Downloaded {
            bytes: bytes.len() as u64,
            sha256: "test-sha".into(),
        })
    }
}

#[derive(Default)]
struct PluginDeps {
    requires: &'static [&'static str],
    conflicts: &'static [&'static str],
    optional: &'static [&'static str],
}

fn plugin_txt(name: &str, deps: &PluginDeps) -> String {
    let mut out = format!("name \"{name}\"\nversion 1.0\n");
    let mut block = String::new();
    for (label, names) in [
        ("requires", deps.requires),
        ("optional", deps.optional),
        ("conflicts", deps.conflicts),
    ] {
        if !names.is_empty() {
            block += &format!("\t{label}\n");
            for n in names {
                block += &format!("\t\t\"{n}\"\n");
            }
        }
    }
    if !block.is_empty() {
        out += "dependencies\n";
        out += &block;
    }
    out
}

fn make_zip(path: &Path, wrapper: &str, plugin_txt: &str) {
    let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
    let opts = SimpleFileOptions::default();
    zip.add_directory(format!("{wrapper}/data/"), opts).unwrap();
    zip.start_file(format!("{wrapper}/data/x.txt"), opts)
        .unwrap();
    zip.write_all(b"ship stuff").unwrap();
    zip.start_file(format!("{wrapper}/plugin.txt"), opts)
        .unwrap();
    zip.write_all(plugin_txt.as_bytes()).unwrap();
    zip.finish().unwrap();
}

fn entry(name: &str, version: &str) -> CatalogEntry {
    CatalogEntry {
        name: name.into(),
        authors: "Test Author".into(),
        homepage: "https://example.com".into(),
        license: "MIT".into(),
        version: version.into(),
        short_description: format!("{name} does things"),
        description: None,
        url: format!("https://example.com/{name}/{version}.zip"),
        icon_url: Some(format!("https://example.com/{name}/icon.png")),
        autoupdate: None,
    }
}

struct World {
    dir: tempfile::TempDir,
    config: PathBuf,
    catalog: Arc<Mutex<Vec<CatalogEntry>>>,
    fetcher: Arc<ZipFetcher>,
    progress: Arc<Mutex<Vec<DownloadProgress>>>,
    shell: Shell,
}

impl World {
    /// A shell with one user-added install (config dir only) selected and the catalog
    /// loaded, so every test starts from "the app just opened".
    fn new() -> Self {
        let world = Self::bare();
        world
            .shell
            .add_custom_install(Some(world.config.display().to_string()), None)
            .unwrap();
        world
    }

    /// No install and no catalog loaded yet.
    fn bare() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("endless-sky");
        fs::create_dir_all(install::plugins_dir(&config)).unwrap();
        let catalog: Arc<Mutex<Vec<CatalogEntry>>> = Arc::default();
        let fetcher = Arc::new(ZipFetcher::default());
        let progress: Arc<Mutex<Vec<DownloadProgress>>> = Arc::default();

        let catalog_for_fetch = catalog.clone();
        let progress_sink = progress.clone();
        let deps = Deps {
            data_dir: dir.path().join("app-data"),
            cache_dir: dir.path().join("app-cache"),
            env: None,
            fetcher: fetcher.clone(),
            fetch_catalog: Box::new(move |_| {
                Ok(CatalogFetch {
                    entries: catalog_for_fetch.lock().unwrap().clone(),
                    source: FetchSource::Fresh,
                    fetched_at: SystemTime::now(),
                })
            }),
            fetch_icon: Box::new(|cache_dir, _url| {
                let path = cache_dir.join("icons/fake.png");
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, b"\x89PNG").unwrap();
                Ok(path)
            }),
            detect_game: fake_detect_game,
            query_version: fake_version,
            launch: fake_launch,
            progress: Arc::new(move |p| progress_sink.lock().unwrap().push(p)),
        };
        World {
            config,
            catalog,
            fetcher,
            progress,
            shell: Shell::new(deps),
            dir,
        }
    }

    /// Registers a catalog entry whose download is a zip wrapping `<catalog name>-src`.
    fn add(&self, catalog_name: &str, identity: &str, version: &str, deps: PluginDeps) {
        let e = entry(catalog_name, version);
        let zip = self
            .dir
            .path()
            .join(format!("{catalog_name}-{version}.zip"));
        make_zip(
            &zip,
            &format!("{catalog_name}-src"),
            &plugin_txt(identity, &deps),
        );
        self.fetcher
            .files
            .lock()
            .unwrap()
            .insert(e.url.clone(), zip);
        let mut catalog = self.catalog.lock().unwrap();
        catalog.retain(|c| c.name != catalog_name);
        catalog.push(e);
        drop(catalog);
        self.shell.load_catalog(true).unwrap();
    }

    /// A plugin folder dropped into `plugins/` by hand (unmanaged, decision B).
    fn drop_in(&self, folder: &str, identity: Option<&str>) {
        let dir = install::plugins_dir(&self.config).join(folder);
        fs::create_dir_all(dir.join("data")).unwrap();
        if let Some(name) = identity {
            fs::write(dir.join("plugin.txt"), format!("name \"{name}\"\n")).unwrap();
        }
    }

    fn install(&self, catalog_name: &str) -> CommitView {
        let ticket = self.shell.begin_planning();
        let plan = self.shell.plan_install(&ticket, catalog_name).unwrap();
        assert!(plan.issues.is_empty(), "{:?}", plan.issues);
        self.shell.commit_plan(plan.plan_id, false).unwrap()
    }

    fn set_enabled(&self, identity: &str, on: bool) {
        let ticket = self.shell.begin_planning();
        let plan = if on {
            self.shell.plan_enable(&ticket, identity)
        } else {
            self.shell.plan_disable(&ticket, identity)
        }
        .unwrap();
        self.shell.commit_plan(plan.plan_id, false).unwrap();
    }

    fn state(&self) -> ManagerState {
        self.shell.get_state().unwrap()
    }

    fn plugin(&self, identity: &str) -> InstalledPluginView {
        self.state()
            .plugins
            .into_iter()
            .find(|p| p.identity == identity)
            .unwrap_or_else(|| panic!("{identity} not installed"))
    }

    fn plugins_txt(&self) -> String {
        fs::read_to_string(self.config.join("plugins.txt")).unwrap_or_default()
    }

    fn paths(&self) -> InstallPaths {
        InstallPaths::new(&self.dir.path().join("app-data"), &self.config)
    }

    fn tmp_is_clean(&self) -> bool {
        match fs::read_dir(install::tmp_dir(&self.config)) {
            Ok(entries) => entries.count() == 0,
            Err(_) => true,
        }
    }
}

fn kind(err: &CmdError) -> String {
    serde_json::to_value(err).unwrap()["kind"]
        .as_str()
        .unwrap()
        .to_string()
}

// ---------------------------------------------------------------------------
// Install / commit lifecycle
// ---------------------------------------------------------------------------

#[test]
fn install_with_dependency_end_to_end() {
    let w = World::new();
    w.add("Base-Pack", "Base Pack", "v1", PluginDeps::default());
    w.add(
        "Ships",
        "Ships",
        "v2",
        PluginDeps {
            requires: &["Base Pack"],
            optional: &["Extras"],
            ..Default::default()
        },
    );

    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_install(&ticket, "Ships").unwrap();
    assert_eq!(plan.kind, PlanKind::Install);
    assert!(plan.issues.is_empty());
    assert_eq!(
        plan.steps,
        [
            StepView::Install {
                catalog_name: "Base-Pack".into(),
                identity: "Base Pack".into(),
                folder: "Base-Pack".into(),
                version: "v1".into(),
                dependency: true,
            },
            StepView::Install {
                catalog_name: "Ships".into(),
                identity: "Ships".into(),
                folder: "Ships".into(),
                version: "v2".into(),
                dependency: false,
            },
        ]
    );
    assert_eq!(
        plan.notes,
        [NoteView::OptionalAvailable {
            plugin: "Ships".into(),
            optional: "Extras".into(),
            installed: false,
        }]
    );
    assert_eq!(plan.game_version.as_deref(), Some("0.10.16"));

    let progress = w.progress.lock().unwrap().clone();
    assert!(progress.iter().all(|p| p.plan_id == plan.plan_id));
    for name in ["Base-Pack", "Ships"] {
        let last = progress.iter().rfind(|p| p.catalog_name == name).unwrap();
        assert_eq!(
            Some(last.received),
            last.total,
            "final progress is always sent"
        );
    }

    let commit = w.shell.commit_plan(plan.plan_id, false).unwrap();
    assert_eq!(commit.installed, ["Base Pack", "Ships"]);
    assert!(!commit.game_detection_failed);
    assert!(w.tmp_is_clean());

    let ships = w.plugin("Ships");
    assert!(ships.enabled && ships.managed);
    assert_eq!(ships.update, UpdateView::UpToDate);
    assert_eq!(ships.requires, ["Base Pack"]);
    let records = records::load(&w.paths().records).unwrap();
    assert_eq!(records["Ships"].sha256, "test-sha");

    // Committed plans are gone.
    assert_eq!(
        kind(&w.shell.commit_plan(plan.plan_id, false).unwrap_err()),
        "planExpired"
    );
}

#[test]
fn blocked_commit_keeps_the_plan_until_overridden() {
    let w = World::new();
    w.add("Old", "Old", "1", PluginDeps::default());
    w.add(
        "New",
        "New",
        "1",
        PluginDeps {
            conflicts: &["Old"],
            ..Default::default()
        },
    );
    w.install("Old");

    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_install(&ticket, "New").unwrap();
    assert_eq!(
        plan.issues,
        [IssueView::Conflict {
            a: "Old".into(),
            b: "New".into()
        }]
    );
    assert_eq!(
        plan.resolvable_conflicts,
        ["Old"],
        "never offers disabling the target"
    );

    let err = w.shell.commit_plan(plan.plan_id, false).unwrap_err();
    match &err {
        CmdError::Blocked { issues, .. } => assert_eq!(issues, &plan.issues),
        other => panic!("{other:?}"),
    }
    assert!(w.plugin("Old").enabled, "nothing was written");

    w.shell.commit_plan(plan.plan_id, true).unwrap();
    assert!(w.plugin("Old").enabled && w.plugin("New").enabled);
}

#[test]
fn resolving_a_conflict_disables_the_other_side_instead() {
    let w = World::new();
    w.add("Old", "Old", "1", PluginDeps::default());
    w.add(
        "New",
        "New",
        "1",
        PluginDeps {
            conflicts: &["Old"],
            ..Default::default()
        },
    );
    w.install("Old");

    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_install(&ticket, "New").unwrap();
    assert_eq!(
        kind(&w.shell.resolve_conflict(plan.plan_id, "New").unwrap_err()),
        "invalid"
    );
    let resolved = w.shell.resolve_conflict(plan.plan_id, "Old").unwrap();
    assert!(resolved.issues.is_empty());
    assert_eq!(
        resolved.steps.last(),
        Some(&StepView::Disable {
            identity: "Old".into()
        })
    );

    w.shell.commit_plan(plan.plan_id, false).unwrap();
    assert!(!w.plugin("Old").enabled);
    assert!(w.plugin("New").enabled);
    assert!(
        w.plugins_txt().contains("Old 0"),
        "written as 0, never false"
    );
}

#[test]
fn game_running_refuses_without_consuming_the_plan() {
    let w = World::new();
    w.add("A", "A", "1", PluginDeps::default());
    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_install(&ticket, "A").unwrap();

    GAME_RUNNING.with(|r| r.set(true));
    let err = w.shell.commit_plan(plan.plan_id, false).unwrap_err();
    GAME_RUNNING.with(|r| r.set(false));
    assert_eq!(kind(&err), "gameRunning");
    assert!(w.state().plugins.is_empty());

    w.shell.commit_plan(plan.plan_id, false).unwrap();
    assert_eq!(w.state().plugins.len(), 1);
}

#[test]
fn a_new_plan_supersedes_the_pending_one() {
    let w = World::new();
    w.add("A", "A", "1", PluginDeps::default());
    w.add("B", "B", "1", PluginDeps::default());

    let t1 = w.shell.begin_planning();
    let first = w.shell.plan_install(&t1, "A").unwrap();
    let t2 = w.shell.begin_planning();
    let second = w.shell.plan_install(&t2, "B").unwrap();
    assert_ne!(first.plan_id, second.plan_id);
    assert_eq!(
        kind(&w.shell.commit_plan(first.plan_id, false).unwrap_err()),
        "planExpired"
    );

    w.shell.discard_plan(second.plan_id);
    assert_eq!(
        kind(&w.shell.commit_plan(second.plan_id, false).unwrap_err()),
        "planExpired"
    );
    assert!(
        w.tmp_is_clean(),
        "dropped plans delete their staged downloads"
    );
}

#[test]
fn cancelled_or_superseded_planning_never_becomes_pending() {
    let w = World::new();
    w.add("A", "A", "1", PluginDeps::default());

    let ticket = w.shell.begin_planning();
    w.shell.cancel_planning();
    assert_eq!(
        kind(&w.shell.plan_install(&ticket, "A").unwrap_err()),
        "cancelled"
    );

    // A slow plan finishing after a newer one started is dropped, not offered.
    let slow = w.shell.begin_planning();
    let fast = w.shell.begin_planning();
    assert_eq!(
        kind(&w.shell.plan_install(&slow, "A").unwrap_err()),
        "cancelled"
    );
    let plan = w.shell.plan_install(&fast, "A").unwrap();
    w.shell.commit_plan(plan.plan_id, false).unwrap();
    assert!(w.tmp_is_clean());
}

#[test]
fn a_failed_download_of_the_requested_plugin_is_an_error_not_a_plan() {
    let w = World::new();
    w.catalog.lock().unwrap().push(entry("Unreachable", "1"));
    w.shell.load_catalog(true).unwrap();
    let ticket = w.shell.begin_planning();
    let err = w.shell.plan_install(&ticket, "Unreachable").unwrap_err();
    assert_eq!(kind(&err), "network");
    assert!(err.message().contains("no fixture"), "{err}");
    assert!(w.tmp_is_clean());

    // A failed *dependency* download is still an issue in a reviewable plan.
    w.add(
        "Needs-It",
        "Needs It",
        "1",
        PluginDeps {
            requires: &["Unreachable"],
            ..Default::default()
        },
    );
    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_install(&ticket, "Needs-It").unwrap();
    assert!(matches!(
        plan.issues.as_slice(),
        [IssueView::CatalogDownloadFailed { catalog_name, .. }] if catalog_name == "Unreachable"
    ));
}

#[test]
fn installing_twice_is_refused_in_favor_of_update() {
    let w = World::new();
    w.add("A", "A", "1", PluginDeps::default());
    w.install("A");
    let ticket = w.shell.begin_planning();
    assert_eq!(
        kind(&w.shell.plan_install(&ticket, "A").unwrap_err()),
        "invalid"
    );
    let ticket = w.shell.begin_planning();
    assert_eq!(
        kind(&w.shell.plan_install(&ticket, "Nope").unwrap_err()),
        "notFound"
    );
}

#[test]
fn update_with_a_new_requirement_shows_and_installs_it() {
    let w = World::new();
    w.add("A", "A", "v1", PluginDeps::default());
    w.add("Dep", "Dep", "1", PluginDeps::default());
    w.install("A");

    w.add(
        "A",
        "A",
        "v2",
        PluginDeps {
            requires: &["Dep"],
            ..Default::default()
        },
    );
    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_update(&ticket, "A").unwrap();
    assert!(plan.issues.is_empty(), "{:?}", plan.issues);
    assert!(
        matches!(
            plan.steps.first(),
            Some(StepView::Install { identity, dependency: true, .. }) if identity == "Dep"
        ),
        "the dependency comes first: {:?}",
        plan.steps
    );
    assert!(matches!(plan.steps.last(), Some(StepView::Update { .. })));

    w.shell.commit_plan(plan.plan_id, false).unwrap();
    assert!(w.plugin("Dep").enabled);
    assert_eq!(w.plugin("A").update, UpdateView::UpToDate);
}

#[test]
fn update_is_detected_planned_and_committed() {
    let w = World::new();
    w.add("A", "A", "v1", PluginDeps::default());
    w.install("A");
    w.set_enabled("A", false);

    w.add("A", "A", "v2", PluginDeps::default());
    assert_eq!(
        w.plugin("A").update,
        UpdateView::Available {
            from: "v1".into(),
            to: "v2".into()
        }
    );

    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_update(&ticket, "A").unwrap();
    assert_eq!(
        plan.steps,
        [StepView::Update {
            catalog_name: "A".into(),
            identity: "A".into(),
            folder: "A".into(),
            from: "v1".into(),
            to: "v2".into(),
        }]
    );
    w.shell.commit_plan(plan.plan_id, false).unwrap();
    let a = w.plugin("A");
    assert_eq!(a.update, UpdateView::UpToDate);
    assert!(!a.enabled, "an update keeps the enabled state (decision D)");
}

#[test]
fn update_all_commits_every_outdated_plugin_in_one_plan() {
    let w = World::new();
    w.add("A", "A", "v1", PluginDeps::default());
    w.install("A");
    w.add("B", "B", "v1", PluginDeps::default());
    w.install("B");
    w.set_enabled("B", false);

    w.add("A", "A", "v2", PluginDeps::default());
    w.add("B", "B", "v2", PluginDeps::default());

    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_update_all(&ticket).unwrap();
    assert_eq!(plan.kind, PlanKind::UpdateAll);
    assert_eq!(plan.steps.len(), 2, "{:?}", plan.steps);
    assert!(
        plan.steps.iter().any(
            |s| matches!(s, StepView::Update { identity, from, to, .. } if identity == "A" && from == "v1" && to == "v2")
        )
    );
    assert!(
        plan.steps.iter().any(
            |s| matches!(s, StepView::Update { identity, from, to, .. } if identity == "B" && from == "v1" && to == "v2")
        )
    );

    w.shell.commit_plan(plan.plan_id, false).unwrap();
    assert_eq!(w.plugin("A").update, UpdateView::UpToDate);
    assert_eq!(w.plugin("B").update, UpdateView::UpToDate);
    assert!(w.plugin("A").enabled);
    assert!(
        !w.plugin("B").enabled,
        "update-all keeps each plugin's enabled state, same as a single update"
    );
}

#[test]
fn update_all_with_nothing_outdated_is_an_error_not_an_empty_plan() {
    let w = World::new();
    w.add("A", "A", "v1", PluginDeps::default());
    w.install("A");

    let ticket = w.shell.begin_planning();
    assert_eq!(
        kind(&w.shell.plan_update_all(&ticket).unwrap_err()),
        "invalid"
    );
}

// ---------------------------------------------------------------------------
// Enable / disable / uninstall
// ---------------------------------------------------------------------------

#[test]
fn disabling_a_requirement_is_blocked_and_enabling_offers_the_right_fix() {
    let w = World::new();
    w.add("Lib", "Lib", "1", PluginDeps::default());
    w.add(
        "App",
        "App",
        "1",
        PluginDeps {
            requires: &["Lib"],
            ..Default::default()
        },
    );
    w.install("App");

    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_disable(&ticket, "Lib").unwrap();
    assert_eq!(
        plan.issues,
        [IssueView::RequiredBy {
            plugin: "Lib".into(),
            dependents: vec!["App".into()],
        }]
    );

    w.set_enabled("App", false);
    w.set_enabled("Lib", false);
    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_enable(&ticket, "App").unwrap();
    assert_eq!(
        plan.issues,
        [IssueView::MissingRequirement {
            plugin: "App".into(),
            requires: "Lib".into()
        }]
    );
    assert_eq!(
        plan.fixes,
        [RequirementFixView {
            requires: "Lib".into(),
            enable: Some("Lib".into()),
            install: None,
            candidates: vec![],
        }],
        "an installed-but-disabled requirement is fixed by enabling, not reinstalling"
    );
}

#[test]
fn missing_requirement_offers_a_catalog_install() {
    let w = World::new();
    w.add("Lib", "Lib", "1", PluginDeps::default());
    w.drop_in("Hand-Installed", Some("Hand Installed"));
    // Give the dropped-in plugin a requirement, then disable it so enabling re-checks.
    fs::write(
        install::plugins_dir(&w.config).join("Hand-Installed/plugin.txt"),
        plugin_txt(
            "Hand Installed",
            &PluginDeps {
                requires: &["Lib"],
                ..Default::default()
            },
        ),
    )
    .unwrap();
    fs::write(
        w.config.join("plugins.txt"),
        "state\n\t\"Hand Installed\" 0\n",
    )
    .unwrap();

    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_enable(&ticket, "Hand Installed").unwrap();
    assert_eq!(plan.fixes[0].install.as_deref(), Some("Lib"));
}

#[test]
fn uninstall_flags_unmanaged_plugins_and_removes_the_folder() {
    let w = World::new();
    w.drop_in("Mystery", None);
    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_uninstall(&ticket, "Mystery").unwrap();
    assert!(plan.unmanaged_target);
    assert_eq!(
        plan.steps,
        [StepView::Uninstall {
            folder: "Mystery".into(),
            identity: "Mystery".into()
        }]
    );
    w.shell.commit_plan(plan.plan_id, false).unwrap();
    assert!(w.state().plugins.is_empty());
}

// ---------------------------------------------------------------------------
// Adoption (decision B)
// ---------------------------------------------------------------------------

#[test]
fn unmanaged_plugins_are_adopted_or_offered_for_linking() {
    let w = World::bare();
    w.shell
        .add_custom_install(Some(w.config.display().to_string()), None)
        .unwrap();
    w.drop_in("Jimmys-Ship-Emporium", Some("Jimmy's Ship Emporium"));
    w.drop_in("Dup", None);

    let before = w.state();
    assert!(!before.catalog_loaded);
    assert!(before.adopted.is_empty(), "no adoption without a catalog");
    assert!(
        before
            .plugins
            .iter()
            .all(|p| p.update == UpdateView::Unmanaged)
    );

    w.add(
        "Jimmys-Ship-Emporium",
        "Jimmy's Ship Emporium",
        "v1",
        PluginDeps::default(),
    );
    w.add("Dup!", "x", "1", PluginDeps::default());
    w.add("DUP", "y", "1", PluginDeps::default());
    let state = w.state();
    assert_eq!(state.adopted, ["Jimmys-Ship-Emporium"]);
    let jimmy = w.plugin("Jimmy's Ship Emporium");
    assert!(jimmy.managed);
    assert_eq!(
        jimmy.update,
        UpdateView::Unknown,
        "adopted version is unknown"
    );
    assert_eq!(
        state.unmanaged,
        [UnmanagedView {
            folder: "Dup".into(),
            identity: "Dup".into(),
            candidates: vec!["Dup!".into(), "DUP".into()],
        }]
    );
    assert!(w.state().adopted.is_empty(), "adoption happens once");

    w.shell.adopt_plugin("Dup", "DUP").unwrap();
    assert_eq!(w.plugin("Dup").catalog_name.as_deref(), Some("DUP"));
    assert_eq!(
        kind(&w.shell.adopt_plugin("Dup", "DUP").unwrap_err()),
        "invalid"
    );
}

// ---------------------------------------------------------------------------
// Profiles (decision E)
// ---------------------------------------------------------------------------

#[test]
fn first_state_read_creates_the_default_profile() {
    let w = World::new();
    w.drop_in("Pre", None);
    let profiles = w.state().profiles;
    assert_eq!(profiles.active.as_deref(), Some("Default"));
    assert_eq!(
        profiles.profiles,
        [ProfileSummary {
            name: "Default".into(),
            enabled_count: 1
        }]
    );
}

#[test]
fn switching_profiles_is_a_checked_plan() {
    let w = World::new();
    w.add("X", "X", "1", PluginDeps::default());
    w.add(
        "Y",
        "Y",
        "1",
        PluginDeps {
            conflicts: &["Z"],
            ..Default::default()
        },
    );
    w.add("Z", "Z", "1", PluginDeps::default());
    w.install("X");
    w.install("Y");
    assert_eq!(w.shell.create_profile("  Both ").unwrap(), "Both");
    // Changes made through the manager follow the active profile, so switch to a new one
    // before disabling Y, leaving "Both" as it was.
    w.shell.create_profile("Only X").unwrap();
    w.set_enabled("Y", false);
    assert_eq!(
        kind(&w.shell.create_profile("Both").unwrap_err()),
        "invalid"
    );

    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_apply_profile(&ticket, "Both").unwrap();
    assert_eq!(plan.kind, PlanKind::ApplyProfile);
    assert_eq!(
        plan.steps,
        [StepView::Enable {
            identity: "Y".into()
        }]
    );
    assert!(plan.issues.is_empty());
    let commit = w.shell.commit_plan(plan.plan_id, false).unwrap();
    assert_eq!(commit.enabled, ["Y"]);
    let state = w.state();
    assert_eq!(state.profiles.active.as_deref(), Some("Both"));
    assert!(state.profiles.drift.in_profile_but_disabled.is_empty());
    assert!(w.plugins_txt().contains("Y 1"));

    // A profile enabling a conflicting pair, plus a plugin that isn't installed.
    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_install(&ticket, "Z").unwrap();
    w.shell.commit_plan(plan.plan_id, true).unwrap();
    let mut store = ProfileStore::load(&w.paths().profiles).unwrap();
    store
        .insert(
            "Clash",
            Profile {
                enabled: [
                    ("Y".to_string(), None),
                    ("Z".to_string(), None),
                    ("Gone".to_string(), Some("Gone-Cat".to_string())),
                ]
                .into(),
            },
        )
        .unwrap();
    store.save(&w.paths().profiles).unwrap();
    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_apply_profile(&ticket, "Clash").unwrap();
    assert_eq!(
        plan.issues,
        [IssueView::Conflict {
            a: "Y".into(),
            b: "Z".into()
        }]
    );
    assert_eq!(
        plan.missing,
        [MissingView {
            identity: "Gone".into(),
            catalog_name: Some("Gone-Cat".into())
        }]
    );
    assert_eq!(
        plan.steps,
        [StepView::Disable {
            identity: "X".into()
        }]
    );
    assert_eq!(
        kind(&w.shell.commit_plan(plan.plan_id, false).unwrap_err()),
        "blocked"
    );
}

#[test]
fn apply_profile_conflict_resolved_via_resolve_conflict() {
    let w = World::new();
    w.add(
        "Y",
        "Y",
        "1",
        PluginDeps {
            conflicts: &["Z"],
            ..Default::default()
        },
    );
    w.add("Z", "Z", "1", PluginDeps::default());
    // Installing Y while Z is enabled would already conflict, so install Z and switch it off
    // first: the profile below re-enabling it is what should surface the conflict, not the
    // install.
    w.install("Z");
    w.set_enabled("Z", false);
    w.install("Y");
    w.state(); // creates the "Default" profile
    let mut store = ProfileStore::load(&w.paths().profiles).unwrap();
    store
        .insert(
            "Clash",
            Profile {
                enabled: [("Y".to_string(), None), ("Z".to_string(), None)].into(),
            },
        )
        .unwrap();
    store.save(&w.paths().profiles).unwrap();

    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_apply_profile(&ticket, "Clash").unwrap();
    assert_eq!(
        plan.issues,
        [IssueView::Conflict {
            a: "Y".into(),
            b: "Z".into()
        }]
    );
    assert_eq!(
        plan.resolvable_conflicts,
        ["Y", "Z"],
        "a profile switch has no single root identity to exclude"
    );

    let resolved = w.shell.resolve_conflict(plan.plan_id, "Z").unwrap();
    assert!(resolved.issues.is_empty());
    assert!(
        resolved.steps.is_empty(),
        "Z stays disabled, matching its current state: nothing to do"
    );

    w.shell.commit_plan(plan.plan_id, false).unwrap();
    assert!(w.plugin("Y").enabled);
    assert!(!w.plugin("Z").enabled);
    assert_eq!(w.state().profiles.active.as_deref(), Some("Clash"));
    assert!(w.plugins_txt().contains("Z 0"), "written as 0, never false");
}

#[test]
fn drift_is_reported_and_resolvable() {
    let w = World::new();
    w.add("X", "X", "1", PluginDeps::default());
    w.install("X");
    assert!(w.state().profiles.drift.in_profile_but_disabled.is_empty());

    // The user toggles it off in-game.
    fs::write(w.config.join("plugins.txt"), "state\n\tX 0\n").unwrap();
    assert_eq!(w.state().profiles.drift.in_profile_but_disabled, ["X"]);

    w.shell.update_active_profile().unwrap();
    assert_eq!(w.state().profiles.drift, DriftView::default());
}

#[test]
fn the_active_profile_cannot_be_deleted() {
    let w = World::new();
    w.state();
    w.shell.create_profile("Other").unwrap();
    assert_eq!(
        kind(&w.shell.delete_profile("Other").unwrap_err()),
        "invalid"
    );
    w.shell.delete_profile("Default").unwrap();
    assert_eq!(
        kind(&w.shell.delete_profile("Default").unwrap_err()),
        "notFound"
    );
}

#[test]
fn rename_profile_preserves_contents_and_active_status() {
    let w = World::new();
    w.add("A", "A", "v1", PluginDeps::default());
    w.install("A");
    w.state(); // creates the "Default" profile, active, containing A

    let renamed = w.shell.rename_profile("Default", "Main").unwrap();
    assert_eq!(renamed, "Main");
    let profiles = w.state().profiles;
    assert_eq!(profiles.active.as_deref(), Some("Main"));
    assert_eq!(profiles.profiles.len(), 1);
    assert_eq!(profiles.profiles[0].name, "Main");
    assert_eq!(profiles.profiles[0].enabled_count, 1, "A is still in it");

    assert_eq!(
        kind(&w.shell.rename_profile("Gone", "New").unwrap_err()),
        "notFound"
    );
    w.shell.create_profile("Other").unwrap();
    assert_eq!(
        kind(&w.shell.rename_profile("Other", "Main").unwrap_err()),
        "invalid" // ProfileError::DuplicateName maps to CmdError::Invalid
    );
}

#[test]
fn a_profile_can_be_exported_and_imported_as_a_file() {
    let w = World::new();
    w.add("A", "A", "v1", PluginDeps::default());
    w.install("A");
    w.state(); // creates the "Default" profile containing A

    let file = w.dir.path().join("shared.esmm-profile.json");
    w.shell.export_profile("Default", &file).unwrap();
    assert_eq!(
        kind(&w.shell.export_profile("Gone", &file).unwrap_err()),
        "notFound"
    );

    // Importing the same file twice never collides: the second gets a number.
    assert_eq!(w.shell.import_profile(&file).unwrap(), "Default (2)");
    assert_eq!(w.shell.import_profile(&file).unwrap(), "Default (3)");
    let profiles = w.state().profiles;
    assert_eq!(
        profiles.active.as_deref(),
        Some("Default"),
        "import doesn't switch"
    );
    assert_eq!(profiles.profiles.len(), 3);

    let bad = w.dir.path().join("bad.json");
    std::fs::write(&bad, "not a profile").unwrap();
    assert_eq!(kind(&w.shell.import_profile(&bad).unwrap_err()), "invalid");
}

// ---------------------------------------------------------------------------
// Installs, catalog, launch
// ---------------------------------------------------------------------------

#[test]
fn installs_have_separate_state_and_switching_drops_the_pending_plan() {
    let w = World::new();
    w.add("A", "A", "1", PluginDeps::default());
    w.install("A");
    let first = w.shell.list_installs(false).selected.unwrap();

    let other = w.dir.path().join("flatpak-config");
    let installs = w
        .shell
        .add_custom_install(Some(other.display().to_string()), None)
        .unwrap();
    assert_eq!(installs.installs.len(), 2);
    assert!(installs.installs.iter().all(|i| i.user_added));
    assert_ne!(installs.selected.as_deref(), Some(first.as_str()));
    assert!(
        w.state().plugins.is_empty(),
        "the new install has its own plugins"
    );

    let ticket = w.shell.begin_planning();
    let plan = w.shell.plan_install(&ticket, "A").unwrap();
    w.shell.select_install(&first).unwrap();
    assert_eq!(
        kind(&w.shell.commit_plan(plan.plan_id, false).unwrap_err()),
        "planExpired"
    );
    assert_eq!(w.state().plugins.len(), 1);

    let selected = w.shell.list_installs(false).selected.unwrap();
    assert_eq!(selected, first, "selection persists in settings.json");
    assert_eq!(
        kind(&w.shell.select_install("nope").unwrap_err()),
        "notFound"
    );
}

#[test]
fn custom_install_input_is_validated() {
    let w = World::bare();
    let invalid = |config: Option<&str>, exe: Option<&str>| {
        kind(
            &w.shell
                .add_custom_install(config.map(str::to_string), exe.map(str::to_string))
                .unwrap_err(),
        )
    };
    assert_eq!(invalid(None, None), "invalid");
    assert_eq!(invalid(Some("  "), Some("")), "invalid");
    assert_eq!(invalid(Some("relative/dir"), None), "invalid");
    assert_eq!(
        invalid(Some("/abs"), Some("/definitely/not/a/file")),
        "invalid"
    );
    // Executable-only needs the default config dir, which needs a home dir (env).
    let exe = w.dir.path().join("endless-sky.AppImage");
    fs::write(&exe, "").unwrap();
    assert_eq!(invalid(None, Some(exe.to_str().unwrap())), "invalid");
}

#[test]
fn no_install_means_no_install_errors() {
    let w = World::bare();
    let state = w.state();
    assert!(state.install.is_none());
    assert_eq!(kind(&w.shell.launch_game().unwrap_err()), "noInstall");
    let ticket = w.shell.begin_planning();
    assert_eq!(
        kind(&w.shell.plan_enable(&ticket, "X").unwrap_err()),
        "noInstall"
    );
}

#[test]
fn launch_needs_a_known_way_to_start() {
    let w = World::new();
    assert_eq!(kind(&w.shell.launch_game().unwrap_err()), "invalid");
    let exe = w.dir.path().join("endless-sky-bin");
    fs::write(&exe, "").unwrap();
    w.shell
        .add_custom_install(
            Some(w.config.display().to_string()),
            Some(exe.display().to_string()),
        )
        .unwrap();
    w.shell.launch_game().unwrap();
}

#[test]
fn icons_are_served_only_for_catalog_urls() {
    let w = World::new();
    w.add("A", "A", "1", PluginDeps::default());
    let url = entry("A", "1").icon_url.unwrap();
    assert_eq!(
        w.shell.icon(&url).unwrap(),
        "data:image/png;base64,iVBORw=="
    );
    assert_eq!(
        kind(&w.shell.icon("https://evil.example/x.png").unwrap_err()),
        "notFound"
    );
}

// ---------------------------------------------------------------------------
// Error and view mapping (the frontend's contract)
// ---------------------------------------------------------------------------

#[test]
fn commit_errors_map_to_tagged_command_errors() {
    let issues = vec![Issue::IdentityMismatch {
        expected: "A".into(),
        catalog_name: "A-Cat".into(),
        actual: "B".into(),
    }];
    let json = serde_json::to_value(CmdError::from(CommitError::Blocked(issues))).unwrap();
    assert_eq!(json["kind"], "blocked");
    assert_eq!(
        json["issues"][0],
        serde_json::json!({
            "kind": "identityMismatch",
            "expected": "A",
            "catalogName": "A-Cat",
            "actual": "B",
        })
    );
    assert!(json["message"].as_str().unwrap().contains("1 blocking"));

    assert_eq!(kind(&CommitError::GameRunning.into()), "gameRunning");
    assert_eq!(kind(&CommitError::Io("disk".into()).into()), "io");

    let failed: CmdError = CommitError::InstallFailed {
        report: Box::new(CommitReport {
            enabled: vec!["Dep".into()],
            ..Default::default()
        }),
        folder: "Top".into(),
        error: esmm_core::install::InstallError::NotAPlugin,
    }
    .into();
    let json = serde_json::to_value(&failed).unwrap();
    assert_eq!(json["kind"], "installFailed");
    assert_eq!(json["folder"], "Top");
    assert_eq!(json["partial"]["enabled"], serde_json::json!(["Dep"]));
}

#[test]
fn views_serialize_camel_case_with_kind_tags() {
    let step = StepView::Install {
        catalog_name: "C".into(),
        identity: "I".into(),
        folder: "F".into(),
        version: "v".into(),
        dependency: true,
    };
    assert_eq!(
        serde_json::to_value(step).unwrap(),
        serde_json::json!({
            "kind": "install",
            "catalogName": "C",
            "identity": "I",
            "folder": "F",
            "version": "v",
            "dependency": true,
        })
    );
    let update = serde_json::to_value(UpdateView::Available {
        from: "1".into(),
        to: "2".into(),
    })
    .unwrap();
    assert_eq!(
        update,
        serde_json::json!({"kind": "available", "from": "1", "to": "2"})
    );
    assert_eq!(
        serde_json::to_value(PlanKind::ApplyProfile).unwrap(),
        "applyProfile"
    );
}

// ---------------------------------------------------------------------------
// Through Tauri's IPC: argument names and the error payload as the frontend sees them
// ---------------------------------------------------------------------------

mod ipc {
    use tauri::Manager;
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};
    use tauri::webview::InvokeRequest;

    use super::*;
    use crate::commands::AppState;

    fn invoke(
        webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
        cmd: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, serde_json::Value> {
        get_ipc_response(
            webview,
            InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|body| body.deserialize::<serde_json::Value>().unwrap())
    }

    #[test]
    fn frontend_argument_names_and_error_payloads() {
        let w = World::new();
        w.add("A", "A", "1", PluginDeps::default());
        let config = w.config.clone();
        let world_dir = w.dir;
        // Move the shell into the app; keep the temp dir alive for the test's duration.
        let app = crate::with_commands(mock_builder())
            .build(mock_context(noop_assets()))
            .unwrap();
        app.manage(AppState::new(w.shell));
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();

        let catalog = invoke(
            &webview,
            "load_catalog",
            serde_json::json!({"refresh": false}),
        )
        .unwrap();
        assert_eq!(catalog["entries"][0]["shortDescription"], "A does things");

        let plan = invoke(
            &webview,
            "plan_install",
            serde_json::json!({"catalogName": "A"}),
        )
        .unwrap();
        assert_eq!(plan["kind"], "install");
        let plan_id = plan["planId"].as_u64().unwrap();

        let commit = invoke(
            &webview,
            "commit_plan",
            serde_json::json!({"planId": plan_id, "overrideIssues": false}),
        )
        .unwrap();
        assert_eq!(commit["installed"], serde_json::json!(["A"]));
        assert!(install::plugins_dir(&config).join("A").is_dir());

        let err = invoke(
            &webview,
            "commit_plan",
            serde_json::json!({"planId": plan_id, "overrideIssues": false}),
        )
        .unwrap_err();
        assert_eq!(err["kind"], "planExpired");
        assert!(err["message"].as_str().is_some());

        let state = invoke(&webview, "get_state", serde_json::json!({})).unwrap();
        assert_eq!(state["plugins"][0]["identity"], "A");
        assert_eq!(state["game"], "notRunning");

        let err = invoke(&webview, "plan_install", serde_json::json!({"wrong": "A"})).unwrap_err();
        assert!(
            err.as_str().is_some_and(|e| e.contains("catalogName")),
            "{err}"
        );
        drop(world_dir);
    }
}
