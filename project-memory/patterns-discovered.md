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
