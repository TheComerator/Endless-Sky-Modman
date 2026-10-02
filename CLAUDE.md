# Endless Sky Mod Manager: Project Standards

> **Note:** This file supplements the root CLAUDE.md (`~/projects/CLAUDE.md`). Universal standards (communication style, inquiry protocols, anti-patterns) are defined there. This file covers what's specific to THIS project.

---

## What This Project Does

A desktop mod manager for *Endless Sky* (the open-source 2D space trading/exploration/combat game), modeled on **r2modman**: browse a catalog of community plugins, install/update/remove them, enable/disable them, and switch between profiles, without manually unzipping files into the game's plugin folder.

---

## Competitive Positioning (confirmed by Jon 2026-10-01)

`EndlessSkyCommunity/ESLauncher2` (Rust, GPL-3.0, 69 stars) is an existing, actively-maintained community project that also touches plugins, but its actual focus is managing the *game* itself (installing builds/versions, running multiple instances); plugin install/toggle is one basic feature bolted on, with no evident dependency resolution, conflict detection, identity matching, or profiles. `esmm`'s deliberate differentiation is to go deep on exactly what ESLauncher2 doesn't do: dependency/conflict resolution surfaced clearly, profiles, identity matching, at r2modman-level simplicity and polish. This is a focused, excellent *mod* manager, not a game/version launcher. When a design choice could go either toward "more like a general launcher" or "simpler/more focused like r2modman," prefer the latter.

---

## Catalog Source (confirmed 2026-10-01)

Endless Sky has no Thunderstore-style API, but it has an official machine-readable plugin index:

```
https://raw.githubusercontent.com/endless-sky/endless-sky-plugins/master/generated/plugins.json
```

This is what `endless-sky.github.io/plugins.html` fetches client-side. Flat JSON array, 165 entries as of 2026-10-01. Per-plugin JSON files also exist at `generated/plugins/<name>.json`. Schema per entry:

```json
{
  "name": "Jimmys-Ship-Emporium",
  "authors": "Jimmy Firyh",
  "homepage": "https://github.com/JimmyFiryh/Jimmys-Ship-Emporium",
  "license": "CC-BY-SA-4.0",
  "version": "v0.1.1",
  "shortDescription": "Adds the Navy Glaive.",
  "description": "Simply adds the Glaive, a spicy Republic Navy ship...",
  "url": "https://github.com/JimmyFiryh/Jimmys-Ship-Emporium/archive/refs/tags/v0.1.1.zip",
  "iconUrl": "https://github.com/JimmyFiryh/Jimmys-Ship-Emporium/raw/v0.1.1/icon.png",
  "autoupdate": {
    "type": "tag",
    "url": "https://github.com/JimmyFiryh/Jimmys-Ship-Emporium/archive/refs/tags/$version.zip",
    "iconUrl": "https://github.com/JimmyFiryh/Jimmys-Ship-Emporium/raw/$version/icon.png"
  }
}
```

**The catalog is near-live.** The index repo's `autoupdate.yml` workflow runs hourly, checks each plugin's own repo for new tags/commits, and auto-merges version bumps; `generate.yml` regenerates `plugins.json` hourly. 163 of 165 entries have autoupdate (150 `tag`, 13 `commit`). So the catalog's `version` is accurate to within roughly two hours, and update checking needs no GitHub API calls (no rate limits).

Quirks to handle, not assume away:
- Icon key is inconsistently cased (`iconUrl` on 150 entries, `iconURL` on 1). Normalize when parsing. Icon is optional; fall back to a generic image.
- `version` formats vary wildly: `v0.1.1`, `0.10.1`, `v1.0.8-ship.merging`, 40-char commit SHAs (13 entries). Never try to order versions; any difference from the installed version means "update available".
- Download URL shapes: 96 GitHub release assets, 43 GitHub tag archives, 25 other GitHub archives (incl. commit SHAs), and 4 non-GitHub hosts (codeberg.org x2, git.nixnet.services, bitbucket.org). Don't assume GitHub.
- Sizes vary: most are under 3 MB, but Mega Freight is 166 MB.

---

## Game-Side Plugin Mechanics (confirmed 2026-10-01, read from endless-sky source)

**Install paths.** Config dir = `SDL_GetPrefPath(nullptr, "endless-sky")`; user plugins live in `<config>/plugins/`:
- Windows: `%APPDATA%\endless-sky\plugins\`
- Linux: `~/.local/share/endless-sky/plugins/` (respects `$XDG_DATA_HOME`)
- macOS: `~/Library/Application Support/endless-sky/plugins/`
- The `-c/--config <path>` CLI flag overrides the config dir entirely.

There is also a global plugin folder next to the game's own resources (`Files.cpp`: `resources / "plugins"`). This manager targets the user folder only.

**Discovery order (`GameData.cpp`):** global unzipped, global `.zip`, user unzipped, user `.zip`. Unzipped plugins take precedence over zipped ones. Duplicate names: first one loaded wins, later ones are skipped with a warning.

**Valid plugin:** a folder containing at least one of `data/`, `images/`, `shaders/`, `sounds/` (`PluginManager::IsPlugin()`).

**Plugin identity:** the `name` field in `plugin.txt` if present, otherwise the folder name. Dependencies reference these names.

**`plugin.txt`** (optional; engine's DataNode syntax, not JSON): `name`, `about` (repeatable; legacy fallback `about.txt`), `version`, `authors` (list), `tags` (list), `dependencies` (`"game version"`, `requires`, `optional`, `conflicts`). Real example (Mega Freight):

```
name "Mega Freight"
version 1.0.0010101000100100111
authors
	1010todd
dependencies
	"game version" 0.10.13.1
	optional
		"Mega Freight: Weapon Pack"
	conflicts
		"Mini Freight"
```

**The game does NOT enforce dependencies.** It parses them and shows them in the in-game description, but nothing acts on `requires`, `conflicts`, or `game version`. The manager is the only enforcement layer.

**Enabled state:** `<config>/plugins.txt`, a `state` block of `<name> <state>` pairs. A plugin not listed defaults to **enabled**. The in-game Preferences UI rewrites this file from memory when the user toggles plugins.

**The state must be written as `1`/`0`, never `true`/`false`.** The game writes it with a plain `ostream << bool` (which prints 1/0) and reads it back with `DataNode::Value()`, a numeric parse. A `true` token reads as 0, so writing booleans would silently disable every plugin. Enforced and tested in `esmm-core/src/plugin_state.rs`.

**DataNode text format rules (`DataFile.cpp`/`DataWriter.cpp`):** indentation defines nesting (writer uses tabs; reader accepts tabs or spaces but warns on mixing); `#` starts a comment line; tokens separated by whitespace; a token with a `"` is wrapped in backticks, a token with whitespace or a backtick (or empty) is wrapped in `"`, otherwise bare. Match this exactly when writing `plugins.txt`.

**What real plugin zips look like** (11 sampled 2026-10-01):
- Every zip wraps the plugin in ONE top-level folder, usually named `<repo>-<version>` (e.g. `A-Coalition-At-War-0.10.4.2.0`). Sometimes the inner name doesn't even match the release (`HyperHue-Studios-2.0` inside a v2.1 zip).
- 6 of 11 have no `plugin.txt` (only `about.txt`), so the game would use the folder name as identity.
- Catalog name and `plugin.txt` name can differ (`Jimmys-Ship-Emporium` vs `Jimmy's Ship Emporium`).

**Launch targets (confirmed):** Steam app id `404410` (`steam://rungameid/404410`); Flathub app id `io.github.endless_sky.endless_sky`; no Snap package exists.

**Executable names (confirmed 2026-10-01 from `CMakeLists.txt` `OUTPUT_NAME`, `cd_release.yaml`, `steam/docker-compose.yml`, `utils/build_appimage.sh`, and the Flathub manifest):** `endless-sky` on Linux (native, Steam Linux depot, Flatpak `command`, AppImage); `Endless Sky.exe` on Windows (also the Steam Windows depot, so also what runs under Proton); `Endless Sky` on macOS (`Endless Sky.app/Contents/MacOS/Endless Sky`). All are at most 15 bytes, so Linux's truncated process name (`/proc/<pid>/comm`) matches them exactly. Used by `game_state::detect_game_process`.

---

## Tech Stack (locked in 2026-10-01)

- **Framework:** Tauri (Rust backend, web frontend in the OS native webview). Chosen over Python/PySide6 and Electron for small installers (~3-10 MB vs 100 MB+) and an r2modman-like UI.
- **Backend:** Rust. Narrow scope: HTTP, zip extraction, text parsing, process launch.
- **Frontend:** React + TypeScript (reversible choice, picked for Tauri ecosystem maturity).
- **Packaging/CI:** GitHub Actions (`tauri-action`) for Windows/macOS/Linux builds; this VPS can only build and test Linux locally.
- **Rejected:** Python + PySide6 (clunkier solo cross-platform packaging, less r2modman-like UI); Electron (same UI approach, much bigger installers).

---

## v1 Feature Scope (locked in 2026-10-01)

**Baseline:** browse catalog; install; uninstall; enable/disable without uninstalling.

**Also in v1:** local profiles; dependency/conflict warnings; update checking; launch game button.

**Out of scope for v1** (don't build without re-confirming):
- In-app config editing (doesn't map onto Endless Sky's static-content plugins)
- Manual install of a plugin not in the catalog (drag-and-drop zip/folder)
- The game's global plugin folder (user folder only)
- Loading plugins as `.zip` without extracting (game supports it, but it complicates updates)
- Android version of the game
- Shareable profile codes: needs a hosted backend (store a profile behind a short code, resolve it later, the role Thunderstore plays for r2modman). Shelved 2026-10-01 as premature for this community's size; revisit if real users ask for it.

---

## Design Decisions (locked in 2026-10-01)

**A. Identity and install layout.** Strip the zip's wrapper folder and install under a stable folder named after the sanitized catalog name. Version must never appear in the folder name, otherwise every update changes the plugin's identity and orphans its enabled state. The manager keeps its own install record per managed plugin (catalog name, `plugin.txt` name, installed version, source URL, our own SHA-256 of the download, folder name) in the manager's app-data directory, never inside the plugin folder. Dependency matching resolves by `plugin.txt` name first, then folder name, against installed plugins and the catalog.

**B. Unmanaged plugins** (already in `plugins/`, not installed by the manager). If there's a catalog match, adopt it automatically. If there's no match, ask the user. Unmanaged plugins can always be enabled/disabled and are never deleted silently.

**C. Dependency resolution.** Dependency data only exists inside the download, so: download to a staging folder, parse its `plugin.txt`, resolve `requires` recursively (cycle guard), then present ONE plan before anything is committed ("installing X also installs Y and Z; X conflicts with W, which is enabled"). Commit atomically.
- Problems (required plugin not in catalog, game version too old, conflict with an enabled plugin, uninstalling something others require): **warn and block, with an explicit user override to proceed anyway.**
- Optional dependencies: shown, never forced.
- Implemented in `esmm-core/src/resolve.rs` (pure checks, no I/O) and `esmm-core/src/manager.rs` (orchestration: scanning, planning, committing).

**Identity matching (`resolve::match_catalog`).** `requires`/`conflicts`/`optional` name plugin *identities* (the `plugin.txt` `name`, else the folder name), not catalog names, and the two often differ (`Jimmys-Ship-Emporium` vs `Jimmy's Ship Emporium`). Matching an identity against the catalog, in order:
- **Exact:** an install record already maps this identity to a catalog name, or a catalog entry's name equals the identity exactly.
- **Likely:** normalize both sides (lowercase, keep only alphanumerics: `Jimmy's Ship Emporium` and `Jimmys-Ship-Emporium` both become `jimmysshipemporium`) and compare. Exactly one catalog entry matching = `Likely`; several = `Ambiguous` (never resolved automatically). Never fuzzy/edit-distance matched.
- A `Likely` match **must be confirmed after download**: if the downloaded plugin's actual `plugin.txt` identity doesn't equal the identity that was required, that's an `IdentityMismatch` issue, not a silent success. The plugin still gets staged and installed under its real identity; the specific requirement that named it just stays unmet (and is reported as such) unless something else in the final state also satisfies it.

**What `game version` means (undocumented upstream; here's what we found and what we assume).** Read `source/GameVersion.{h,cpp}` and `source/Plugin.h` directly: the engine's `GameVersion` class only ever *formats* a version (`ToString()`: `major.minor.release.patch`, `-alpha` suffixed for a non-full-release build) and has no parser and no comparison operators at all. `Plugin::PluginDependencies::gameVersion` is a bare `std::string`, stored and dropped straight into the plugin's description text (`Plugin::CreateDescription`) -- never parsed, never compared, anywhere in the engine. The wiki (`CreatingPlugins`) says only: `"game version"`: *"the game version(s) that this plugin is expected to function with"* -- genuinely ambiguous between minimum/exact/range. **We treat it as a minimum required version**, since that's the only reading under which "update your game" can ever resolve the problem; this is our own inference, not confirmed upstream, and worth revisiting if Jon or upstream ever clarifies it.
- Our own comparator (`resolve::compare_game_versions`), used only by this manager: dotted numeric components, differing lengths padded with trailing zeros (so `0.10.0` < `0.10.13.1`), and a `-alpha` build ranks just below the same numbers without the suffix (so `0.11.4.0-alpha` < `0.11.4.0`). An unparseable string on either side, or the installed game's version simply being unknown (detection failed or hasn't run), is `GameVersionUnknown` -- never guessed.

**Conflict resolution options.** A `Conflict` between the new plan and an already-installed, enabled plugin can be resolved either by overriding (keep both enabled, the general escape hatch for every blocking issue) or, specifically for conflicts, by `InstallPlan::disable_to_resolve(identity)`, which adds a "disable that plugin" step to the plan and removes the conflict issue outright -- no override needed. A conflict between two plugins newly introduced *within the same plan* works the same way.

**Commit behavior.** `manager::commit` (and its `commit_enable`/`commit_disable`/`commit_uninstall`/`commit_update` siblings) all: refuse up front if blocking issues remain and no override was given; check `GameProcess` and refuse before touching *anything* if it's `Running` (plugins.txt can't survive the game exiting over it); otherwise install each staged plugin in order (dependencies first; each individual install is already atomic via `install::install`) and write install records, `plugins.txt`, and the active profile together at the end. If one step in a multi-step plan fails partway, commit stops there and persists records/`plugins.txt`/the profile for exactly what succeeded before the failure -- never a half-applied plugin, but also not a full multi-plugin transaction/rollback system. `CommitContext::detect_game` is a swappable function pointer (production wires in the real `game_state::detect_game_process`) purely so tests can simulate "the game is running" deterministically.

**D. Update checking.** Compare the install record's version against the catalog's `version` by string equality. Any difference = update available. Updating replaces the folder atomically and keeps the enabled state (stable folder name from A makes this work).

**E. Profiles.** A profile records **only which plugins are enabled**, not versions. Because unlisted plugins default to enabled in `plugins.txt`, applying a profile must write an explicit enabled/disabled entry for every installed plugin (written as `1`/`0`). Applying a profile that references a plugin that isn't installed offers to install it. Flag drift when the live `plugins.txt` no longer matches the active profile (e.g. user toggled plugins in-game).
- Profiles live in the manager's app-data dir (JSON), never in the game's config dir.
- Each enabled entry stores the game identity and, when known from install records, the catalog name, so a missing plugin can be installed from the catalog.
- First run: if no profiles exist, the current effective state is snapshotted into an active profile named "Default".
- A plugin installed while a profile is active is added to that profile as enabled.
- Drift is only about installed plugins: enabled but not in the profile, or in the profile but disabled. A profile entry that isn't installed is "missing", not drift. The UI offers three resolutions: update the active profile to match (keeps missing entries), restore the profile (apply), or save the current state as a new named profile.
- Profile names: empty or whitespace-only names and duplicates are rejected; names are trimmed.
(Confirmed by Jon 2026-10-01; implemented in `esmm-core/src/profiles.rs`.)

**F. `plugins.txt` safety.** Detect a running game and block writes while it runs (the game would overwrite our changes). Back up before every write; write atomically (temp file + rename).
- If the process list can't be read, the write proceeds and the outcome says detection failed, so the UI can warn.
- The backup is `<config>/plugins.txt.esmm-bak`, overwritten on each write. Nothing is ever written to `plugins/`.
(Implemented in `esmm-core/src/game_state.rs`.)

**G. Downloads.** Streaming with progress and cancel. Zip-slip protection (reject entries escaping the target), size limits, HTTPS only. Validate the extracted folder with the game's own `IsPlugin` rule before committing.

**H. Game install detection: all install types supported**, the same coverage expectation as the Valheim tooling: native per-OS paths, Steam (Windows/macOS/Linux), Flatpak, standalone/manual installs, and custom `-c` config paths. Auto-detect, let the user override, allow multiple installs. Flatpak's config dir is confirmed as `~/.var/app/io.github.endless_sky.endless_sky/data/endless-sky/`: the Flathub manifest (`io.github.endless_sky.endless_sky.json`) sets no `--filesystem` override and no custom environment, so Flatpak's own automatic XDG redirection applies and `XDG_DATA_HOME` resolves to `<app>/data` inside the sandbox. Game version is read via `endless-sky --version`, confirmed in `source/main.cpp`'s `PrintVersion()`: it writes `Endless Sky ver. <version>` to stderr for every install type (native, Steam, standalone). From 0.11.0 onward the version string is `GameVersion::ToString()` (always `major.minor.release.patch`, `-alpha` suffixed for non-full-release builds, confirmed in `source/GameVersion.cpp`); before 0.11.0 it was a hardcoded literal with an inconsistent digit count (e.g. `0.10.0`, `0.10.13.1`). The parser only looks for the `Endless Sky ver.` prefix, so it doesn't depend on the digit count either way.

**I. Offline and caching.** Cache catalog and icons with HTTP ETags. Enable/disable and profiles must work fully offline.
- The cache lives in a cache directory the caller supplies (the manager's own app-data, never the game's own directories): `catalog-body.json` and `catalog-meta.json` (the ETag and fetch time) at its root, icons under an `icons/` subdirectory.
- `fetch_cached` sends the stored ETag as `If-None-Match`. A 304 reuses the cached body (`FetchSource::NotModified`); a 200 parses, replaces the cache, and returns `FetchSource::Fresh`; a failed request falls back to the cached body as `FetchSource::Offline { error }` when a cache exists, otherwise errors.
- A 200 with a body that fails to parse as the catalog schema is treated as a catalog-source bug: it returns `Err` and leaves the existing good cache (body and ETag) untouched, never overwritten with something unparseable.
- A missing or corrupt cache file (either the body or the metadata) is treated as no cache at all; it never crashes, and the next fetch just goes out with no `If-None-Match`.
- Icon URLs embed the plugin version, so they're cached forever, keyed by the sha256 hex digest of the URL (with a plain image extension kept on the filename when the URL has one). A URL already on disk is never re-fetched. `prune_icons` deletes cached icons whose URL is no longer referenced by the current catalog.
- All HTTP goes through an injectable `HttpGet` trait so tests never touch the network; `fetch_cached`/`cached_icon` wire in the real ureq-backed client, `fetch_cached_with`/`cached_icon_with` take any `&dyn HttpGet` and carry the actual logic. HTTPS-only and a hard body size cap (20 MB catalog, 5 MB icon) apply the same as `download.rs`. Writes are atomic (temp file + rename) via `crate::files::write_atomic`.
(Implemented in `esmm-core/src/catalog.rs`; tests in `esmm-core/tests/catalog_cache.rs`, including one `#[ignore]`d live test against the real catalog URL.)

**J. Code layout.** A pure Rust core library with no Tauri dependency (catalog, DataNode parser/writer, installer, resolver, profiles), testable on this Linux box with fixtures from real plugins; a thin Tauri shell exposing commands; a React UI on top.

**K. Tauri shell and UI architecture (designed 2026-10-01).** Items marked *(inference)* are our own calls where the decisions above were silent; none has been confirmed by Jon yet, so revisit any that feel wrong.
- **Layering.** `app/src-tauri/src/shell.rs` is a plain Rust service (`Shell`) that holds every behavior; `commands.rs` is thin async `#[tauri::command]` wrappers over it; `views.rs` and `error.rs` are the serializable contract. No Tauri type appears in `Shell`, so it's tested directly against temp dirs, a fake `Fetcher` and a fake catalog closure, with `fn`-pointer seams (`Deps`) for game detection, `--version` and launching. One extra test drives real commands through `tauri::test`'s mock IPC to pin the argument names and the error payload exactly as the frontend sends and receives them.
- **Command surface.** Catalog: `load_catalog(refresh)`, `get_icon(url)`. Installs (H): `list_installs(refresh)`, `select_install(key)`, `add_custom_install(configDir?, executable?)`, `remove_custom_install(key)`, `game_status`, `launch_game`. State: `get_state` (installed plugins with enabled/update status, unmanaged plugins needing a choice, profiles with drift and missing entries, game process), `adopt_plugin(folder, catalogName)`. Plans: `plan_install(catalogName)`, `plan_update(folder)`, `plan_enable(identity)`, `plan_disable(identity)`, `plan_uninstall(folder)`, `plan_apply_profile(name)`, `resolve_conflict(planId, identity)`, `cancel_planning`, `discard_plan(planId)`, `commit_plan(planId, overrideIssues)`. Profiles (no `plugins.txt` write, so no plan): `create_profile(name)`, `update_active_profile`, `delete_profile(name)`. JavaScript passes camelCase argument names; Tauri maps them onto the snake_case Rust parameters.
- **State management.** The game's files and the manager's JSON files are the source of truth and are re-read on every call (they're tiny, and the game rewrites `plugins.txt` behind our back). The shell keeps in memory only what can't be cheaply re-read or can't be serialized: the catalog snapshot, `--version` results per install, the single pending plan, and the in-flight planning ticket. The frontend keeps no model of plugin state at all: it re-fetches `get_state` after every commit, on window focus, and when the 4-second game-process poll sees the game exit.
- **Plan/commit over IPC.** A plan owns staged downloads (`TempDir`s), so it never crosses IPC: `plan_*` stores it in the shell and returns a `PlanView` (steps, `issues`, `notes`, `fixes`, `resolvableConflicts`, `missing`, `gameVersion`, `unmanagedTarget`) keyed by a `planId`. At most one plan is pending; starting another supersedes it (dropping its staged downloads), and a stale id gets `planExpired`. `commit_plan` checks the refusals the user can act on (blocking issues without an override, the game running) *before* taking the plan, so overriding or retrying after closing the game doesn't re-download anything; any other outcome consumes the plan. `resolve_conflict` wraps `InstallPlan::disable_to_resolve` and returns the updated view. A pending plan always commits against the install it was planned for, and selecting another install discards it.
- **Planning, cancel and progress.** Each planning run gets a `Ticket` (monotonic id plus cancel flag) and runs on its own thread; the command awaits it with `tokio::select!` against an abort channel, so cancel returns immediately even when ureq is stuck inside `read()` (the known limitation below). The abandoned thread's result is dropped by `Shell::finish` because its ticket is no longer current, which also deletes its staged download; planning never takes the commit lock, so an abandoned thread can't block anything. `fetcher::PlanFetcher` wraps the real `HttpFetcher`, substitutes the ticket's cancel flag, and emits a `download-progress` event (throttled to 100 ms, final value always sent) tagged with the plan id; the frontend ignores events from older ids.
- **Errors.** Every command returns `CmdError`, a tagged `{ kind, message, ... }`: `noInstall`, `gameRunning`, `blocked { issues }`, `planExpired`, `cancelled`, `notFound`, `invalid`, `installFailed { folder, partial }`, `network`, `io`. `error.rs` maps `CommitError`, `WriteError`, `ProfileError`, `RecordsError` and `CatalogError` onto it.
- **Types.** ts-rs exports every view and `CmdError` to `app/src/bindings/` whenever `cargo test` runs (`TS_RS_EXPORT_DIR` is set in `.cargo/config.toml`). The generated files are committed; a diff after `cargo test` means the contract changed.
- **Per-install state** *(inference)*. `records.json` and `profiles.json` live under `<app-data>/installs/<16 hex of sha256(config dir)>/` (plus a `config-dir.txt` note for humans), so each game config dir (a native vs a Flatpak install) has its own records and profiles, while installs sharing a config dir share them, matching how they share `plugins/`. `<app-data>/settings.json` holds the selected install key and user-added installs (a corrupt one falls back to defaults rather than refusing to start). The catalog/icon cache (I) is shared at `<app-cache>/catalog/`. App identifier `io.github.thecomerator.esmm`, so on Linux that's `~/.local/share/io.github.thecomerator.esmm/` and `~/.cache/io.github.thecomerator.esmm/`.
- **Install selection (H).** Detected installs plus user-added ones (a config dir, an executable, or both) are listed in Settings; the choice persists; the default is the first detected install.
- **Review vs. instant** *(inference)*. Install, update, uninstall and profile switches always show the plan dialog. An enable/disable with no issues and no note about a missing optional dependency commits straight from the toggle. A blocked plan needs a ticked "proceed anyway" checkbox before its commit button works.
- **Fix actions.** A `Conflict` in an install plan offers "Disable X instead" (`disable_to_resolve` exists only on `InstallPlan`, so enable/profile plans offer override only). A `MissingRequirement` offers "Enable X first" when X is installed but disabled, else "Install X first" for its catalog match (or one button per candidate when ambiguous). This deliberately doesn't use `EnablePlan::requirement_fixes`, which offers a catalog reinstall even when the requirement is installed and merely disabled. The same buttons are the "install this new requirement" step that `plan_update`'s known risk below called for.
- **Profile switch is a checked plan** *(inference)*. `plan_apply_profile` runs `check_state` + `check_requirements` over the state the profile would produce, so switching into a profile with a conflict or unmet `requires` is blocked-with-override like any other change. Plugins the profile enables that aren't installed are listed, and installable from a banner after switching. Creating a profile saves the current state as a new active profile; the active profile can't be deleted; renaming (added 2026-10-02, `profiles::rename`) is validated the same way creation is (empty/whitespace rejected, duplicates rejected, trimmed) and preserves contents and active status.
- **Adoption (B)** *(inference about the UX)*. Automatic adoption runs inside `get_state` once the catalog is loaded. "Ask the user" is non-modal: plugins with several catalog matches get a "Link" picker on the Installed view; plugins with no match just carry an "Unmanaged" badge and stay fully toggleable. Uninstalling an unmanaged plugin is allowed, but its plan dialog warns that it wasn't installed by the manager (explicit, so not "silent").
- **Smaller calls.** Installing a catalog entry that already has a record is refused in favor of update ("Reinstall latest" is `plan_update`, which is also how an adopted plugin's unknown version gets pinned). Icons go to the webview as `data:` URLs from `get_icon`, which only serves URLs listed in the loaded catalog (so it can't be used as a general fetch proxy), loaded lazily as they scroll into view. Commit-SHA versions are shown as 7 characters. `launch_game` spawns the game and reaps it on a background thread.
- **Update all** (added 2026-10-02). `plan_update_all` plans every plugin with `UpdateView::Available` the same way `plan_update` plans one, then combines all of their steps/issues/notes into a single `PendingPlan::UpdateAll` for one review dialog, backed by `esmm-core`'s new `commit_update_all` (same partial-failure honesty as the install-plan `commit`: a mid-batch install failure still persists every update before it). A plugin that fails to download is skipped and reported as a `CatalogDownloadFailed` issue rather than aborting the batch - that issue still blocks the whole thing until overridden, same as any other issue. The button only appears on the Installed view when at least one update is available.

## L. File-based logging (added 2026-10-02)

One global `tracing_subscriber`, writing to a daily-rolling file under the platform's standard app log directory (`app.path().app_log_dir()`), is the only subscriber registered for the whole process. `tracing_log::LogTracer` bridges any `log`-crate output (Tauri/webview internals) into the same subscriber, so both ecosystems land in one file rather than needing two sinks. `esmm-core` only depends on the `tracing` facade, never a subscriber, so it stays Tauri-independent; the app crate (`src-tauri/src/logging.rs`) owns the actual sink.

Covers: every `plan_*`/`commit_*` function's start and outcome in `esmm-core` (info on success, warn on blocking issues, error on failure), download start/completion/failure, catalog fetch outcomes, game launch attempts, and every Tauri command's entry/failure at the IPC boundary (`commands.rs`'s `blocking`/`plan` helpers). Deliberately excludes per-byte download progress and per-poll process checks - that volume would make the file useless for actually debugging something.

*(inference)*: rotates daily via `tracing_appender::rolling::daily`, with no automatic pruning of old files - an accepted v1 gap for a single-user desktop app's realistic log volume, worth revisiting if it ever matters. Logging starts inside `.setup()`, so Tauri's own pre-setup bootstrap isn't captured - judged not worth chasing for a desktop app's startup window.

---

## Project Map

```
EndlessSky/
├── Cargo.toml                       # Rust workspace
├── README.md
├── CLAUDE.md                        # You are here
├── project-memory/
│   ├── lessons-learned.md
│   └── patterns-discovered.md
└── crates/
    └── esmm-core/                   # Pure Rust core, no Tauri dependency
        ├── src/
        │   ├── datanode.rs          # DataNode parser/writer (mirrors DataFile.cpp / DataWriter.cpp)
        │   ├── plugin_meta.rs       # plugin.txt -> PluginMeta (name, version, deps...)
        │   ├── plugin_state.rs      # plugins.txt text parse/write (1/0 states)
        │   ├── catalog.rs           # catalog JSON parse + fetch (ureq), plus the ETag catalog/icon cache (decision I)
        │   ├── download.rs          # streaming HTTPS download: progress, cancel, size cap, SHA-256
        │   ├── install.rs           # zip extract (zip-slip safe), plugin root finding, atomic install/uninstall
        │   ├── records.rs           # the manager's install records (JSON, app-data dir)
        │   ├── game_state.rs        # game-running detection, safe plugins.txt read/write with backup
        │   ├── profiles.rs          # profiles: snapshot, apply, drift, default profile (JSON, app-data dir)
        │   ├── game_install.rs      # install detection (native/Steam/Flatpak/custom) and launch (decision H)
        │   ├── resolve.rs           # pure dependency/conflict/game-version checks, identity matching (decision C)
        │   ├── manager.rs           # orchestration with I/O: scan, plan (install/enable/disable/uninstall/update), commit, adoption (decision C)
        │   └── files.rs             # internal: atomic writes and JSON load/save
        └── tests/
            ├── real_fixtures.rs     # tests against real captured files
            ├── install.rs           # installer tests with zips built on the fly
            ├── game_install.rs      # install detection against fake homes/roots in temp dirs
            ├── catalog_cache.rs     # catalog/icon cache tests against a fake HttpGet, plus one #[ignore]d live test
            ├── manager.rs           # planning/commit tests end to end against a fake Fetcher serving zips built on the fly
            └── fixtures/            # catalog snapshot, 5 real plugin.txt files, game-install/ (VDF/ACF samples)
```

```
EndlessSky/
├── .cargo/config.toml               # sets TS_RS_EXPORT_DIR so `cargo test` regenerates app/src/bindings
└── app/                             # Tauri shell + React UI (decision K)
    ├── package.json                 # npm scripts: dev, build (tsc + vite), test (vitest), tauri
    ├── src-tauri/                   # crate `esmm` (lib `esmm_lib`), a workspace member
    │   ├── tauri.conf.json          # window, CSP, bundle config; identifier io.github.thecomerator.esmm
    │   ├── capabilities/default.json # core, opener (homepage links), dialog:allow-open (pick install paths)
    │   └── src/
    │       ├── lib.rs               # builder, plugins, managed state, command registration
    │       ├── commands.rs          # #[tauri::command] wrappers; abandonable planning thread
    │       ├── shell.rs             # Shell: all behavior, pending plan, catalog snapshot, install selection
    │       ├── fetcher.rs           # PlanFetcher: per-plan cancel flag + throttled progress events
    │       ├── logging.rs           # global tracing subscriber: daily-rolling file (decision L)
    │       ├── settings.rs          # settings.json, install keys, per-config-dir state paths
    │       ├── views.rs             # serializable views (ts-rs exported)
    │       ├── error.rs             # CmdError and the mapping from core errors (ts-rs exported)
    │       └── tests.rs             # shell lifecycle tests + one mock-IPC test
    └── src/
        ├── bindings/                # GENERATED by ts-rs on `cargo test`; committed, never hand-edited
        ├── api.ts                   # typed invoke() wrappers
        ├── format.ts                # plain-language issue/step/note wording (+ format.test.ts)
        ├── hooks/                   # useManager (data + refresh), usePlanFlow (plan -> review -> commit), useToasts
        ├── components/              # PlanDialog, PluginIcon, Toasts
        ├── views/                   # BrowseView, InstalledView, ProfilesView, SettingsView
        ├── App.tsx                  # layout, top bar (install, profile, launch), global banners
        └── styles.css
```

**Repo:** `git@github.com:TheComerator/Endless-Sky-Modman.git` (private, created 2026-10-01 as EndlessSky, renamed 2026-10-01). Same convention as JoyForge: solo committer, small atomic commits straight to `main`, never force-push. Repo-local identity `TheComerator <thecomerator@gmail.com>`. `gh` CLI is installed at `~/.local/bin/gh` and logged in as TheComerator.

**Dev environment:** Rust installed per-user via rustup (`source ~/.cargo/env`). Build with `CARGO_BUILD_JOBS=2` to keep memory down while Valheim is running. Run tests with `cargo test`; the live network test with `cargo test -- --ignored`. Keep `cargo clippy --all-targets` and `cargo fmt --check` clean.

**App dev environment:** Node v20.20.2 / npm 10.8.2 via nvm (already installed per-user at `~/.nvm`; nothing was installed for this project). From `app/`: `npm install`, `npm run build` (typecheck + Vite), `npm test` (vitest), `npm run tauri dev`, `npm run tauri build`. `cargo test` at the root also builds and tests the app crate (it needs the WebKitGTK dev libraries, which are installed) and regenerates `app/src/bindings/`. This VPS has no display: to see the real app, build it (`npm run tauri build -- --debug --no-bundle`), then run `target/debug/esmm` under `Xvfb` *inside `dbus-run-session`* (see lessons-learned), take screenshots with `ffmpeg -f x11grab`, and drive it with `xdotool`. Use a throwaway `HOME` with a fake `~/.local/share/endless-sky/plugins/` so detection finds a "Standalone" install and nothing real is touched.

---

## Current Status

- **Last worked on:** 2026-10-02 (app slice 2: file-based logging (decision L), an "update all" button backed by `esmm-core`'s new `commit_update_all`, and profile rename - plus the pending core work from slice 1's session, `resolve.rs`/`manager.rs` (decision C), got committed)
- **Stage:** v1 feature-complete on Linux: browse/search the live catalog, install with recursive dependencies, uninstall, enable/disable, update checking (one at a time or all at once) and updating, profiles (switch, create, rename, delete, drift resolution, missing-plugin install), game install detection/selection/custom installs, launch button, file-based logging for after-the-fact debugging. All mutations go through one plan-review dialog that surfaces decision C's issues before anything is written. Verified by driving the real app under Xvfb against the live catalog (slice 1): a real install (download, SHA-256 in the record, `plugins.txt` written as `1`/`0`), adoption of a dropped-in plugin, a blocked enable with three issue kinds, and drift detection plus restore. Slice 2's new pieces (update-all, rename, logging) are covered by Rust + frontend tests but haven't had their own from-the-webview Xvfb pass yet.
- **CI (confirmed working 2026-10-02):** `.github/workflows/release.yml` builds all three platforms via `tauri-action` on `workflow_dispatch`/push to `main`, publishing a draft prerelease GitHub Release. First real run (`36890180599`) went green on Windows, macOS, and Linux with no fixes needed, producing `.msi`/`.exe` (Windows), `.dmg`/`.app.tar.gz` (macOS), `.deb`/`.rpm`/`.AppImage` (Linux). No code signing (unsigned, matching ESLauncher2's own approach at this stage — SmartScreen/Gatekeeper warn on first launch, expected). This closes the "set up GitHub Actions for Windows/macOS builds" item.
- **Windows setup verified, first real-world run complete (2026-10-01):** first-ever build/test on Windows (previously Linux-VPS-only; see `project-memory/` for the toolchain-version and test-portability fixes this took). `npm run tauri dev` built and launched the real app on Jon's Windows machine with Endless Sky actually installed via Steam: the app auto-detected the Steam install, browsing the live catalog worked, Jon downloaded and installed a real plugin (Better Vanilla Pirates) through the manager, and launching the game from the app showed the plugin active in-game (Plugins screen, enabled). This is the first confirmation on a real install, closing the "first real-world run on a machine with the game installed" item below for the install-detect/browse/install/launch path specifically; drift detection, uninstall, updates, profiles and the Flatpak/custom-install paths are still only unit-tested.
- **Windows setup verified (2026-10-01):** `cargo test -p esmm-core` (136 tests), `cargo fmt --check`, and `cargo clippy --all-targets` are all clean; the frontend (`npm run build`, `npm test`) is clean too. Required `rustup update stable` (project's rustc was below Tauri 2.12's floor) and two small `esmm-core` test fixes for Windows portability: a missing `#[cfg(unix)]` gate on a test added after the existing convention (`crates/esmm-core/tests/manager.rs`), and a zip-slip test's expected-path assertion that hardcoded a 1-byte Unix root strip instead of deriving it from path components (`crates/esmm-core/tests/install.rs`) — the actual sandboxing logic was already correct on Windows; only the test's own math was wrong. Full detail in `project-memory/lessons-learned.md` and `project-memory/patterns-discovered.md`. Known, not-worth-chasing limitation: `cargo test -p esmm` (the Tauri shell crate's own unit tests) cannot run via plain `cargo test` on Windows at all, because its test harness binary lacks the manifest that `tauri_build` only embeds in the real app binary (`TaskDialogIndirect`/`comctl32.dll` requires it) — exercise that crate's behavior via `npm run tauri dev` instead.
- **Next steps (app):** have Jon review decision K's and L's *(inference)* items.
- **Manual Windows UI pass, slice 2 (2026-10-02):** profile rename and the enable/disable toggle both verified against the real app and its log file (`rename_profile`, `plan_enable`/`commit_plan` entries, a genuine `load_catalog` → "not modified" network round-trip). Found and fixed a real gap: clicking "Check for updates" when nothing was out of date gave no feedback at all — `loadCatalog`'s button just flipped "Checking…" and back with no result, Jon correctly read as "did this even check online?". Fixed in `app/src/hooks/useManager.ts` (`refreshState` now returns the fetched state; `loadCatalog` shows an "All mods are up to date." toast when a user-requested refresh finds zero updates, never on the silent load-on-mount refresh). This closes the "Xvfb pass over slice 2's update-all/rename/logging UI" item, done directly against the real Windows app instead.
- **App icon replaced (2026-10-02):** the default Tauri placeholder icon is gone. New source art at `app/src-tauri/icons/` (regenerated via `npm run tauri icon`) is a ship-with-engine-trail illustration on a deep-space gradient. Jon supplied three raster/vector versions while narrowing in on the final one; the one actually used has a real alpha-transparent rounded-corner cutout (confirmed by sampling corner pixels: alpha 0 at the literal corners, alpha 255 inside, matching the rounded-square mask) rather than an opaque square, which matters for `.icns`/platform icon masking. Unused mobile (iOS/Android) icon sets that the generator also produces by default were deleted — this project has no mobile target.
- **"Fix" buttons now resume the original action (2026-10-02):** `usePlanFlow` tracks the request behind the currently-reviewed plan; starting a new request while in review phase (always a fix sub-action, since the dialog is the only place that happens from) stashes it to resume once the fix's own commit succeeds, cleared on cancel or on the fix itself failing. Chains naturally if a fix needs a fix of its own. No new frontend test infra added for this (the project has none for React hooks yet, only `format.ts`'s pure-logic tests) -- verified by code review + `tsc`; worth a manual click-through next time the app's open. Closes the last "known gaps (app)" item below.
- **Conflict auto-resolve extended to enable/profile plans (2026-10-02):** `EnablePlan` gets its own `disable_to_resolve` (mirrors `InstallPlan`'s); `ApplyProfile` (no core-owned plan struct -- the shell builds it inline from `PluginStates`) has `resolve_conflict` flip the target identity's bit directly and recompute `steps`/`issues`/`notes` via a new `apply_profile_fields` helper shared with `plan_apply_profile`, so the two can't drift apart. `Pending::resolvable_conflicts` covers both now; a profile switch has no single "root" identity to exclude (either conflict side is a legitimate target), unlike install/enable. No frontend changes needed -- `PlanDialog`'s conflict UI was already generic over plan kind, gated only on `resolvableConflicts` being populated. Closes that half of the "conflicts... can only be overridden" known gap.
- **Duplicate-identity installs now blocked (2026-10-02):** installing a catalog plugin whose identity matches an existing folder (typically an unmanaged plugin the catalog matched ambiguously, decision B, so it couldn't auto-adopt) used to create a second folder with the same identity -- the game only loads the first one, so the second silently never loaded. New `Issue::DuplicateIdentity`, checked in `manager.rs`'s `finish_staging` against every installed/unmanaged plugin (covers the root install and any newly-resolved dependency), blocks with the usual override escape hatch -- proceeding anyway is now an explicit choice, never silent. `IssueView`'s TypeScript binding was hand-edited to match ts-rs's output, since `cargo test -p esmm` (which regenerates it) can't run on Windows at all (the `TaskDialogIndirect`/`comctl32` manifest issue, documented above) -- worth a real `cargo test` on Linux to confirm zero diff.
- **Three more roadmap items closed (2026-10-02):** plugin license now shown as a badge in the catalog browser (`BrowseView.tsx`, `.card-title` given `flex-wrap` so a long SPDX string like `ALL-RIGHTS-RESERVED` can't overflow the card) — closes the "future ideas" item below. Log files older than 14 days are now pruned once at startup (`logging.rs`'s new `prune_old_logs`, tested with injected `now`/file mtimes and verified against the real app: a fake 2026-09-01 log file was gone on restart, current-day files survived) — closes that known gap. Also fixed, found during the manual Windows UI pass: deleting installed plugins then switching to a profile that still listed them packed one "Install X" button per missing plugin into a banner that could grow unbounded; it's now a real row list (icon, name, one "Install" button) with a "Download all" button that installs them one at a time, pausing on any that needs a review dialog before moving to the next (`App.tsx`'s `Banners`).
- **Known limitation:** ureq 3.4.2 has no per-read or idle timeout, so a download that stalls mid-read blocks inside `read()` and can't be cancelled from inside `download_to` (the cancel flag is only checked between reads). The app layer must run downloads on a thread it can abandon. `manager.rs` inherits this: a `Fetcher` impl that needs live, interactive cancel must run on its own abandonable thread; `plan_install` itself doesn't expose per-call progress (a `Fetcher` impl can own its own progress callback/cancel flag instead). The app handles both: decision K's abandonable planning thread and `PlanFetcher`.
- **Steam-as-Flatpak now detected (2026-10-02):** confirmed by reading the actual Flathub manifest and `steam_wrapper.py` (not guessed): unlike most Flatpak apps, Steam's own manifest overrides `XDG_DATA_HOME` to `~/.var/app/com.valvesoftware.Steam/.local/share` (not the generic Flatpak auto-redirect to `.../data`), so its library lives at `.../.local/share/Steam`. Added as a `steam_roots()` candidate (`game_install.rs`); the existing native-vs-Proton config-dir logic needed no change, since a found native Linux executable already resolves the normal native config dir regardless of which library it came from. One *(inference)* left stated plainly in the module doc comment: that Steam's separate game-launching runtime (Pressure Vessel) bind-mounts the real host home for game saves, which wasn't directly confirmable from source (that runtime is a separate, more opaque system from the Flatpak manifest/wrapper) but matches wide community usage. AppImages still require the user to add them manually via `GameInstall::standalone` -- acceptable for now.
- **Updates now resolve a newly-added `requires` (2026-10-02):** `plan_update` walks the new version's requirements with the same `PlanBuilder::resolve_requirement` recursion `plan_install` uses (diamonds staged once, cycles terminate, an installed-but-disabled match is enabled rather than reinstalled), only when the plugin is currently enabled. The result lives in `UpdatePlan::new_requirements`; `commit_update`/`commit_update_all` install it first (new installs enabled like any install; the updated plugin's own enabled state is still never touched) with the same partial-failure honesty as `commit`. The old whole-state `check_requirements` pass stays as a safety net, deduplicated against the walk's issues. The shell shows these as dependency steps ahead of the update step (`requirement_steps`). Tested in `esmm-core` (catalog-supplied, installed-but-disabled, and unsatisfiable cases) plus one shell test that can't run on Windows.
- **Known risk (resolve/manager):** `InstallPlan::disable_to_resolve` doesn't re-validate whether disabling that plugin now breaks something else that required it -- acceptable for a first pass (conflict targets are usually leaf plugins) but worth revisiting once there's real usage.
- **License:** MIT, locked 2026-10-02 (matches Jon's JoyForge). `LICENSE` at the repo root; `license.workspace = true` on both crates.
- **Roadmap / future ideas (not scheduled):** none open right now — the license-badge idea (confirmed 2026-10-01 against the upstream RFC, `endless-sky/rfcs`'s `0001-plugin-index.md`, which documents the catalog's `license` field) shipped 2026-10-02, above.

---

*Endless Sky Mod Manager, following Threshold 2.0 project standards.*
