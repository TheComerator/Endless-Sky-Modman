# Endless Sky Mod Manager v0.2.0

**New: share your plugin setups as files.**

## What's new

- **Share profiles.** On the Profiles page, **Export…** saves a profile (a set of enabled plugins) as a small `.esmm-profile.json` file you can post or send to a friend. **Import profile…** adds one someone sent you. Importing never changes your game by itself: switch to the profile like any other, and the manager shows the usual review first and offers to install any plugins you're missing. A shared file only names plugins; it can't contain download links or run anything, and the manager refuses files that are oversized or malformed.
- **The app's version is always visible** next to its name in the top bar.
- **The "Browse" tab is now "Browse Plugins"**, matching what the game itself calls them.
- Includes everything from v0.1.2 (macOS game version detection, a signed macOS build, big plugins like High DPI installing, and built-in updates).

## Download

| System | File |
|---|---|
| Windows | `Endless.Sky.Mod.Manager_0.2.0_x64-setup.exe` (recommended), or the `.msi` |
| macOS | `Endless.Sky.Mod.Manager_0.2.0_aarch64.dmg` (Apple Silicon Macs only; no Intel build yet) |
| Linux | the `.AppImage`, `.deb` or `.rpm` |

Already have the app? It will offer this update itself. The other files (`.sig`, `.tar.gz`, `latest.json`) are for the built-in updater; you don't need them.

## Known caveats

- **Unsigned installers.** Windows SmartScreen will warn ("More info → Run anyway"). On macOS, if the app is reported as "damaged" on first launch, run `xattr -cr "/Applications/Endless Sky Mod Manager.app"` once in Terminal, then open it. Code signing is planned.
- **Profiles don't record versions.** Someone importing your profile gets the current version of each plugin from the official catalog. A plugin that isn't in the catalog is listed as unavailable.
- **Tested on:** Windows 11 and an Apple Silicon Mac, both with Steam installs of Endless Sky 0.11. Linux builds are produced automatically but have had little hands-on testing, so reports from Linux are especially welcome.

## Feedback

Please open an issue, including the log (see the README for its location).
