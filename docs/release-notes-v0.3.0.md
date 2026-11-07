# Endless Sky Mod Manager v0.3.0

This release adds a heads-up for plugins that override each other.

## What's new

- **Overlap warnings.** When you install, update or enable a plugin, or switch profile, the review window now tells you if the change would make one plugin override another. For example, if two plugins both redefine `ship "Hornet"`, you see which one loads last (and so wins), how many things are affected, and a few examples. Warnings are grouped by pair of plugins and only cover what the change itself causes, so overlaps you already have aren't repeated every time you toggle something.
  - It is a heads-up, never a block: many overlaps are intended (a patch plugin, for example).
  - Plugins that only add to a list another plugin also sets (a star system's links, for example) are marked as probably harmless.
  - Everything is read on your computer; no plugin file is changed. Results are remembered per plugin, so later checks are fast.
  - You can turn it off in **Settings → Overlap warnings**.
- **Only one copy opens.** Launching the manager again now brings the window that's already open to the front instead of starting another copy. (Reported on Linux; see the caveat below.)
- Quieter log file: the game-running check is no longer written every few seconds, and the README now lists where the log is on Windows, macOS and Linux.

## Download

| System | File |
|---|---|
| Windows | `Endless.Sky.Mod.Manager_0.3.0_x64-setup.exe` (recommended), or the `.msi` |
| macOS | `Endless.Sky.Mod.Manager_0.3.0_aarch64.dmg` (Apple Silicon Macs only; no Intel build yet) |
| Linux | the `.AppImage`, `.deb` or `.rpm` |

Already have the app? It will offer this update itself. The other files (`.sig`, `.tar.gz`, `latest.json`) are for the built-in updater; you don't need them.

## Known caveats

- **Overlap warnings are a guide, not a verdict.** The manager judges overlaps from the plugin files; it can't know a plugin author's intent, and list-style settings can be flagged even when they combine fine.
- **Linux: multiple windows.** One Linux user reported the app opening several copies. This release stops a second copy from opening, but the underlying cause isn't confirmed. If it still misbehaves for you, please open an issue and attach the log (see the README for its location).
- **Unsigned installers.** Windows SmartScreen will warn ("More info → Run anyway"). On macOS, if the app is reported as "damaged" on first launch, run `xattr -cr "/Applications/Endless Sky Mod Manager.app"` once in Terminal, then open it. Code signing is planned.
- **Profiles don't record versions.** Someone importing your profile gets the current version of each plugin from the official catalog.
- **Tested on:** Windows 11 and an Apple Silicon Mac, both with Steam installs of Endless Sky 0.11. Linux builds are produced automatically but have had little hands-on testing, so reports from Linux are especially welcome.

## Feedback

Please open an issue, including the log (see the README for its location).
