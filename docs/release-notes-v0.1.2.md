# Endless Sky Mod Manager v0.1.2

Fixes found by the first people to try v0.1.0 and v0.1.1, and the first release that updates itself.

## What's new

- **Mac: the game's version is now detected.** On macOS the manager read the version by running the game, which Steam's copy refused. Plugins that need a minimum game version used to be blocked until you forced the install. The version is now read from the game's own `Info.plist`.
- **Mac: signed with an ad-hoc signature**, which Apple Silicon Macs require. First launch should now give a normal "unidentified developer" prompt instead of "damaged and can't be opened".
- **Big plugins install.** The download limit was 512 MB, which blocked High DPI (794 MB). It is now 2 GB, and the unpacked limit is 6 GB.
- **Easier to diagnose.** If the game's version can't be read, the log now says why.
- **Built-in updates.** The app checks GitHub for a newer signed version at startup and offers it with one click. It never installs without your say-so, and you can turn the check off in Settings → Updates. v0.1.0 and v0.1.1 already update to this version this way.

## Download

| System | File |
|---|---|
| Windows | `Endless.Sky.Mod.Manager_0.1.2_x64-setup.exe` (recommended), or the `.msi` |
| macOS | `Endless.Sky.Mod.Manager_0.1.2_aarch64.dmg` (Apple Silicon Macs only; no Intel build yet) |
| Linux | the `.AppImage`, `.deb` or `.rpm` |

The other files (`.sig`, `.tar.gz`, `latest.json`) are for the built-in updater; you don't need them.

## Known caveats

- **Unsigned installers.** Windows SmartScreen will warn ("More info → Run anyway"). On macOS the app may still need `xattr -cr "/Applications/Endless Sky Mod Manager.app"` run once in Terminal on first launch if macOS reports it as damaged. Code signing is planned.
- **Tested on:** Windows 11 and an Apple Silicon Mac, both with Steam installs of Endless Sky 0.11. Linux builds are produced automatically but have had little hands-on testing, so reports from Linux are especially welcome.
- **One assumption to confirm:** for Endless Sky run through *Flatpak* Steam, the app assumes the game saves to the normal home folder. If your plugins end up somewhere else, add that folder under Settings and tell us the path.

## Feedback

Please open an issue, including the log (see the README for its location).

Changes since v0.1.0: see [v0.1.0's notes](https://github.com/TheComerator/Endless-Sky-Modman/releases/tag/v0.1.0) for the full feature list.
