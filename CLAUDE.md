# Endless Sky Mod Manager: Project Standards

> **Note:** This file supplements the root CLAUDE.md (`~/projects/CLAUDE.md`). Universal standards (communication style, inquiry protocols, anti-patterns) are defined there. This file covers what's specific to THIS project.

---

## What This Project Does

A desktop mod manager for *Endless Sky* (the open-source 2D space trading/exploration/combat game), modeled on **r2modman**: browse a catalog of community plugins, install/update/remove them, enable/disable them, and switch between profiles, without manually unzipping files into the game's plugin folder.

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

**D. Update checking.** Compare the install record's version against the catalog's `version` by string equality. Any difference = update available. Updating replaces the folder atomically and keeps the enabled state (stable folder name from A makes this work).

**E. Profiles.** A profile records **only which plugins are enabled**, not versions. Because unlisted plugins default to enabled in `plugins.txt`, applying a profile must write an explicit `true`/`false` for every installed plugin. Applying a profile that references a plugin that isn't installed offers to install it. Flag drift when the live `plugins.txt` no longer matches the active profile (e.g. user toggled plugins in-game).
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
        │   └── files.rs             # internal: atomic writes and JSON load/save
        └── tests/
            ├── real_fixtures.rs     # tests against real captured files
            ├── install.rs           # installer tests with zips built on the fly
            ├── game_install.rs      # install detection against fake homes/roots in temp dirs
            ├── catalog_cache.rs     # catalog/icon cache tests against a fake HttpGet, plus one #[ignore]d live test
            └── fixtures/            # catalog snapshot, 5 real plugin.txt files, game-install/ (VDF/ACF samples)
```

The Tauri shell (`app/`) and React UI are not created yet.

**Repo:** `git@github.com:TheComerator/Endless-Sky-Modman.git` (private, created 2026-10-01 as EndlessSky, renamed 2026-10-01). Same convention as JoyForge: solo committer, small atomic commits straight to `main`, never force-push. Repo-local identity `TheComerator <thecomerator@gmail.com>`. `gh` CLI is installed at `~/.local/bin/gh` and logged in as TheComerator.

**Dev environment:** Rust installed per-user via rustup (`source ~/.cargo/env`). Build with `CARGO_BUILD_JOBS=2` to keep memory down while Valheim is running. Run tests with `cargo test`; the live network test with `cargo test -- --ignored`. Keep `cargo clippy --all-targets` and `cargo fmt --check` clean.

---

## Current Status

- **Last worked on:** 2026-10-01 (core slice 4: game install detection and launch across native/Steam/Flatpak/custom installs (H); all tests, clippy and fmt clean)
- **Stage:** Core library: catalog, DataNode, plugin metadata/state, download, install/uninstall, install records, `plugins.txt` safety, profiles, and game install detection/launch are done. No UI yet.
- **Next steps (core), in order:** dependency resolver and install orchestration that ties download, install, records, and profiles together (C); catalog and icon ETag cache (I); then the Tauri shell.
- **Next steps (app):** install Tauri's Linux system libraries (needs `sudo apt`: `libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev pkg-config`), then scaffold the Tauri shell and React UI.
- **Known limitation:** ureq 3.4.2 has no per-read or idle timeout, so a download that stalls mid-read blocks inside `read()` and can't be cancelled from inside `download_to` (the cancel flag is only checked between reads). The app layer must run downloads on a thread it can abandon.
- **Known gaps (game install detection):** Steam-as-Flatpak is not detected; AppImages require the user to add them manually via `GameInstall::standalone`. Both acceptable for now.
- **Undecided:** project license (left out of Cargo.toml on purpose; Jon's JoyForge is MIT).

---

*Endless Sky Mod Manager, following Threshold 2.0 project standards.*
