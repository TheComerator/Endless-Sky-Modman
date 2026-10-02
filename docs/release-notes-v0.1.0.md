# Endless Sky Mod Manager v0.1.0

The first public release: a desktop app to install, update and switch *Endless Sky* plugins without manual unzipping.

## Highlights

- **Browse and install** from the official plugin catalog. Required plugins are found and installed with it.
- **Profiles** for different playthroughs, with drift detection if you change plugins in-game.
- **Checks before every change**: dependencies, conflicts and game-version requirements are explained in plain language, and a conflict can be fixed by disabling the other plugin in one click.
- **Updates**, one at a time or all together. A new version that needs an extra plugin installs it for you.
- **Finds your game** on Windows, macOS and Linux: Steam (including Flatpak Steam), standalone, AppImage or a custom folder. A Launch button is built in.
- **Careful with your files**: it won't write while the game is running, backs up `plugins.txt` first, and never deletes plugins it didn't install without telling you.

## Download

| System | File |
|---|---|
| Windows | `Endless Sky Mod Manager_0.1.0_x64-setup.exe` (recommended), or the `.msi` |
| macOS | the `.dmg` |
| Linux | the `.AppImage`, `.deb` or `.rpm` |

## Known first-release caveats

- **Unsigned installers.** Windows SmartScreen will warn ("More info → Run anyway"); macOS will ask you to right-click → Open. Code signing is planned.
- **Tested on:** Windows 11 with a Steam install of Endless Sky 0.11. The core logic has an automated test suite. Linux and macOS builds are produced automatically but have had little hands-on testing, so reports from those systems are especially welcome.
- **One assumption to confirm:** for Endless Sky run through *Flatpak* Steam, the app assumes the game saves to the normal home folder. If your plugins end up somewhere else, add that folder under Settings and tell us the path.

## Feedback

Please open an issue, including the log (see the README for its location). Plugin authors: if the manager misreads your plugin's `plugin.txt` dependencies, that's a bug we want to hear about.

This is the first release, so there is no earlier changelog.
