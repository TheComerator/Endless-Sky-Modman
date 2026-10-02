# Patterns Discovered — Endless Sky Mod Manager

Proven solutions worth reusing. Log here as each is found.

---

## Official plugin catalog is a stable, fetchable JSON manifest (2026-10-01)

`endless-sky.github.io/plugins.html` renders client-side from `https://raw.githubusercontent.com/endless-sky/endless-sky-plugins/master/generated/plugins.json` (found by reading the page's own inline `<script>`, not by guessing). This means the mod manager doesn't need to scrape HTML or maintain its own list — it can fetch this URL directly. 165 entries confirmed live. Full schema and quirks documented in `../CLAUDE.md`.

## Game-side plugin mechanics read directly from endless-sky's own source (2026-10-01)

Don't guess at install paths or profile support from how other games' mod loaders work (e.g. BepInEx/Valheim) — endless-sky's source is public and small enough to just read. Confirmed by fetching `source/Files.cpp`, `source/Plugin.{h,cpp}`, `source/PluginManager.{h,cpp}` from `github.com/endless-sky/endless-sky` directly via `curl`:

- User plugin path = `SDL_GetPrefPath(nullptr, "endless-sky")/plugins/` (OS-specific values documented in `../CLAUDE.md`).
- No profile concept exists in the engine - it's one flat per-plugin `enabled` bool, persisted in `<config>/plugins.txt` (engine's own DataNode syntax, a `state` block). This is the single file a "profile" feature in this manager would snapshot/restore - no need to physically move plugin folders the way the Valheim tooling does, because this game already persists the enabled bit separately from the plugin's files on disk.
- A plugin's own optional `plugin.txt` (same DataNode syntax, not JSON) is a *different* schema from the online catalog's JSON - don't conflate them when writing a parser. `plugin.txt` is what carries `requires`/`optional`/`conflicts` dependency data, which the catalog JSON does not have at all.

## Read the game's writer AND reader before writing any game file (2026-10-01)

For `plugins.txt`, the writer (`ostream << bool`) and reader (`DataNode::Value`, numeric) together reveal that states must be `1`/`0`. Looking at either side alone isn't enough: the struct field is a `bool`, so `true`/`false` looks natural, and the game would accept it silently (with only a log warning) while treating every plugin as disabled. For any file the manager writes that the game reads, trace both sides in the game source and pin the format with a test.

## Sample real plugin downloads before designing install logic (2026-10-01)

Downloading 11 real plugins (mixed URL types: release assets, tag archives, commit archives, Bitbucket) exposed the versioned wrapper folder inside every zip, the missing `plugin.txt` in 6 of 11, and catalog/plugin.txt name mismatches. None of this was visible from the catalog JSON or the game source alone, and it drove design decision A (stable install folder names). Keep a small fixture set of real plugin zips for tests rather than synthetic ones.

## Find a game's process name in its build and packaging files, not its title (2026-10-01)

The executable name differs per platform and comes from build config, not the product name: Endless Sky's `CMakeLists.txt` sets `OUTPUT_NAME` to `endless-sky` (Linux), `Endless Sky` (Windows, giving `Endless Sky.exe`) and `Endless Sky` (macOS bundle). Then check every distribution channel actually ships that binary: `.github/workflows/cd_release.yaml` (Steam depots for each OS), `steam/docker-compose.yml`, `utils/build_appimage.sh`, and the Flathub manifest's `command`. Also check the 15-byte limit on Linux process names (`/proc/<pid>/comm`); a longer name would need prefix matching. A Windows exe under Proton/Wine shows up on Linux under its `.exe` name.

## Make filesystem failure paths testable with a read-only parent dir (2026-10-01)

`rename(src, dst)` needs write permission on `src`'s parent. Putting the staged folder inside a `0o555` dir makes the install swap fail AFTER the old copy has been moved out, which exercises the restore path without mocks. Probe-write first and skip if it succeeds (root ignores permissions). See `failed_swap_restores_the_old_version` in `tests/install.rs`.

## Factor network streaming loops over `impl Read` to test them offline (2026-10-01)

`download.rs`'s `copy_limited` takes a generic reader, so the size cap with no Content-Length and mid-download cancel are unit-tested with an in-memory reader that returns 100-byte chunks; no TLS test server needed.

## Inject a `Fetcher` trait to test multi-step download orchestration offline (2026-10-01)

`manager.rs`'s planning (`plan_install` et al.) downloads a whole `requires` tree, not just one file, so it takes a `&dyn Fetcher` (one method: `fetch(entry, dest, progress, cancel) -> Result<Downloaded, String>`) instead of calling `download::download_to` directly. Tests implement `Fetcher` over a `HashMap<url, local zip path>` and just copy bytes; this made the full dependency-chain/diamond/cycle/conflict/update test suite (`tests/manager.rs`) run with zero network access and in well under a second. Build the test zips with the real `zip` crate (already a workspace dependency) and real DataNode `plugin.txt` syntax, not stub strings, the same way `tests/install.rs` does.

## Sabotage one specific staged plugin's rename without affecting its siblings by chmod'ing its own staging folder, found via its known wrapper name (2026-10-01)

To test a multi-step commit failing partway (one plugin's install fails after an earlier one in the same plan already succeeded), you can't just make `plugins_dir` read-only -- that blocks every rename into it, not just the one you want to fail. Each staged plugin lives in its own private `TempDir` under `.esmm-tmp/`, and `manager::StagedPlugin`'s path field is private, so from outside the crate: recursively search `.esmm-tmp/` for a directory literally named after the zip's own wrapper folder (which the test fixture already names deterministically, e.g. `"A-src"`), then `chmod 0o555` *its parent* before calling `commit`. Same read-only-parent trick as `install.rs`'s `failed_swap_restores_the_old_version`, just aimed at a specific sibling in a bigger plan instead of the only staged plugin. See `find_dir_named` and `mid_commit_failure_leaves_records_and_plugins_txt_consistent` in `tests/manager.rs`.

## Give `commit`-style functions a swappable `fn() -> T` field for state that's normally read from the live system (2026-10-01)

`manager::commit` must refuse before touching anything if the game process is `Running`, but the real check (`game_state::detect_game_process`) reads the live process list -- nothing a test can control. `CommitContext` carries `detect_game: fn() -> GameProcess` (defaulted to the real function by `CommitContext::new`), and a test just sets the field to a `fn always_running() -> GameProcess { GameProcess::Running }`. A plain function pointer (not a closure trait object) is enough since tests only need fixed return values, and it keeps the context `Copy`-friendly and simple.

## Keep the Tauri layer a thin wrapper over a plain, testable service (2026-10-01)

`app/src-tauri/src/shell.rs` (`Shell`) holds every behavior and knows nothing about Tauri; `commands.rs` only moves calls onto the right thread and returns `Result<_, CmdError>`. Everything the shell does to the outside world comes in through a `Deps` struct (the `Fetcher`, a catalog-fetching closure, and `fn` pointers for game detection, `--version` and launching), so `src/tests.rs` drives whole plan -> commit lifecycles against temp dirs in well under a second with no webview. Add one test through `tauri::test`'s mock runtime (`mock_builder()` + `get_ipc_response` with an `InvokeRequest` carrying `INVOKE_KEY`) on top: it's the only thing that catches a renamed command argument, because Tauri silently maps JavaScript's `planId`/`catalogName` onto `plan_id`/`catalog_name`. Register commands in one `with_commands(builder)` function shared by `run()` and that test so they can't diverge.

## Generate the frontend's TypeScript types from the Rust views with ts-rs (2026-10-01)

Derive `ts_rs::TS` with `#[ts(export)]` on every type that crosses IPC; ts-rs honors serde's `rename_all`, `tag` and `rename_all_fields`, so the TypeScript matches the JSON exactly (tagged enums become discriminated unions the UI can `switch` on). The bindings are written when `cargo test` runs; point them at the frontend with a workspace `.cargo/config.toml` `[env] TS_RS_EXPORT_DIR = { value = "app/src/bindings", relative = true }` so it works from any directory in the repo. Mark `u64` fields `#[ts(type = "number")]` (ts-rs defaults to `bigint`, which `JSON.parse` never produces). Commit the generated files; a diff after `cargo test` is the signal that the contract changed.

## Make a slow, possibly-stalled operation cancellable by abandoning its thread, with ticket ids (2026-10-01)

ureq can block inside `read()` forever, so a cancel flag alone can't stop a stalled download. The planning command spawns a dedicated `std::thread` and awaits it with `tokio::select!` against a `oneshot` abort channel; cancelling sends on (or simply replaces, which drops) the abort sender, and the command returns at once. The abandoned thread finishes whenever it finishes, and `Shell::finish` only stores its result if the run's `Ticket` (a monotonic id plus a cancel flag) is still the current one; otherwise the result is dropped, which also deletes its staged `TempDir`. The thread never holds the commit lock, so a stuck one can't block later operations. The same increasing id lets the frontend ignore progress events from abandoned runs (`event.planId < minPlanId`).

## Per-test control of a `fn() -> T` seam with a thread-local (2026-10-01)

A `fn`-pointer seam (here `Deps::detect_game`) can't capture per-test state, and a `static` would race between tests running on parallel threads. When the code under test calls the seam on the test's own thread (commit refusal checks do), back it with a `thread_local!` `Cell<bool>` that each test flips: `GAME_RUNNING.with(|r| r.set(true))`. See `game_running_refuses_without_consuming_the_plan` in `app/src-tauri/src/tests.rs`.

## See and drive a Tauri app on a display-less VPS (2026-10-01)

`npm run tauri build -- --debug --no-bundle` (about a minute once dependencies are built), then: `Xvfb :99 -screen 0 1280x860x24 &`, `DISPLAY=:99 dbus-run-session -- target/debug/esmm &` (the private bus is required, see lessons-learned), screenshot with `ffmpeg -f x11grab -video_size 1200x800 -i :99 -frames:v 1 shot.png`, and click/type with `xdotool mousemove X Y click 1` / `xdotool type`. Point `HOME` at a throwaway dir with a fake `~/.local/share/endless-sky/plugins/` (folders with a `data/` subfolder and an optional `plugin.txt`) so detection finds a "Standalone" install and the real home is never touched; the live catalog and real plugin downloads still work through it.

## Set up and verify the project on Windows for the first time (2026-10-01)

Prerequisites beyond Rust/Node: MSVC Build Tools (`vswhere.exe -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64`) and the WebView2 runtime (`HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}`), both of which ship with a normal Windows 11 + VS Build Tools install. Full verification sequence: `rustup update stable` (see lessons-learned for the version floor), `cargo test -p esmm-core` for the real logic coverage, `cargo fmt --check` and `cargo clippy --all-targets` for the two project-mandated lint gates, then `npm install` / `npm run build` / `npm test` in `app/` for the frontend. All green on a first attempt (after the version-floor and `#[cfg(unix)]` fixes in lessons-learned) confirms the whole non-GUI stack is sound on Windows; actually exercising the app window still means `npm run tauri dev`.

## Derive a cross-platform "root stripped" expected path from `Path::components()`, never a hardcoded slice length (2026-10-01)

`zip_slip_and_symlink_entries_stay_inside_staging` (`tests/install.rs`) builds a fake absolute path and asserts where the sanitizer's "strip the root, keep the rest" behavior lands it. The original assertion sliced off 1 byte (`&absolute[1..]`), true only for a Unix root (`/`); a Windows root is 3 bytes (`C:\`), so the same test failed on Windows with a nonsensical expected path, even though the actual sandboxing logic (`install.rs`'s `enclosed_name()` + `Component::Normal`-only check) was already correct and safe on both platforms. Fix: `Path::new(absolute).components().filter_map(|c| match c { Component::Normal(s) => Some(s), _ => None }).collect::<PathBuf>()` reconstructs the same "only the normal segments" path the production sanitizer produces, on any OS, with no hardcoded root length. When a test's *expected value* encodes path semantics, derive it the same structural way the code under test does, rather than string-slicing.

## Diagnose a Windows `STATUS_ENTRYPOINT_NOT_FOUND` by comparing `dumpbin /imports` against a working binary's manifest requirements (2026-10-01)

When a native exe crashes before `main()` with no console output and exit code `-1073741511` (0xC0000139), that's the loader failing to resolve a function in the IAT — not a logic bug. `dumpbin /dependents` (from VS Build Tools, under `VC\Tools\MSVC\<ver>\bin\Hostx64\x64\`) lists every statically-linked DLL; `dumpbin /imports <exe> | Select-String -Context 0,N <dll>` lists exactly which functions are imported from it. Cross-reference an unusual-looking one (here, `TaskDialogIndirect` from `comctl32.dll`) against what's publicly known to require a manifest/SxS redirection — confirms the cause without needing a debugger. Reproducing with a stripped-down `PATH` (`system32` only) first rules out DLL-shadowing as a red herring before chasing the manifest theory.

## Hand-edit a ts-rs TypeScript binding when Windows can't regenerate it, matching the exact output format (2026-10-02)

Adding a new `IssueView` enum variant (for `DuplicateIdentity`) should regenerate `app/src/bindings/IssueView.ts` via `cargo test -p esmm`, but that can't run at all on Windows (`STATUS_ENTRYPOINT_NOT_FOUND`, see the `dumpbin` entry above). Fix: read the current generated file, note its exact style (`{ "kind": "...", field: type, }` unions, one line, `rename_all_fields = "camelCase"` turning `existing_folder` into `existingFolder`), and hand-append a new union member in that same shape. Verified correct by `tsc` type-checking the frontend's actual usage against it, not just by eyeballing. Still worth a real `cargo test` on Linux at some point to confirm the hand-edit produces zero diff against what ts-rs would generate.

## Confirm a Flatpak app's actual data path by reading its real manifest and wrapper script, never assume the generic auto-redirect (2026-10-02)

Most Flatpak apps (including this project's own Endless Sky packaging) get Flatpak's automatic `XDG_DATA_HOME` → `~/.var/app/<id>/data` redirection with no manifest changes needed. Steam's Flatpak (`com.valvesoftware.Steam`) does not: its manifest sets `FLATPAK_STEAM_XDG_DIRS_PREFIX=~/.var/app/com.valvesoftware.Steam`, and `steam_wrapper.py` joins that with a *hardcoded* `.local/share` (not `data`) before Steam starts — landing its library at `.../​.local/share/Steam`, not the generic `.../data/Steam` a wrong-by-analogy guess would produce. Found by fetching the real manifest (`raw.githubusercontent.com/flathub/<id>/master/<id>.yml`, note `.yml` not `.yaml`) and wrapper script directly (`WebFetch`, or `gh api repos/flathub/<id>/contents/` to find exact filenames first when unsure), the same standard this project already holds itself to for its own Flatpak detection. One edge this didn't fully resolve: whether a *launched game's* process (via Steam's separate, more opaque Pressure Vessel container runtime) sees the real host home or the Flatpak-sandboxed one — left as a stated `(inference)` in the code rather than silently assumed, since it genuinely wasn't confirmable from the manifest/wrapper source alone.

## Make a blocking read interruptible by moving it to a worker thread behind a channel (2026-10-02)

When a library offers no idle timeout and no cancellable read (ureq 3.4.2: only a *total* body deadline, which would kill a big download on a slow link), don't wrap the caller in a thread it must abandon: run the blocking read on its own thread, push chunks through a *bounded* `sync_channel`, and give the consumer a `Read` adapter (`ChannelReader` in `download.rs`) that does `recv_timeout` in ~200 ms slices. Each slice re-checks a cancel flag and accumulates idle time, so both a stall and a cancel return immediately even while the worker is still stuck in the socket read. The bounded channel stops the worker buffering a whole download ahead of a slow consumer; the abandoned worker ends on its own when its socket errors or its send finds no receiver. Test it with a `Read` that sleeps like a dead socket, not with a real network.

## Reuse the install planner's recursion for an update instead of re-implementing it (2026-10-02)

`plan_update` used to run a flat `check_requirements` over the final state and merely report a newly-added requirement. Building a `PlanBuilder`, marking the updated plugin's own identity as seen, and calling `resolve_requirement` for each of its `requires` gives updates install's exact behavior (diamonds staged once, cycles terminate, installed-but-disabled enabled rather than reinstalled) for free. Mark the updated plugin seen *first* so a self-reference can't loop, and only walk when it'll end up enabled.
