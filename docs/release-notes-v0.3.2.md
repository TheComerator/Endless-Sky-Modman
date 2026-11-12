# Endless Sky Mod Manager v0.3.2

A quick fix to the start-up card's quotes, thanks to a player who pointed out that some of them were spoilers.

## What's changed

- **Fewer, safer quotes.** 15 of the 30 quotes on the start-up card were removed because they named species, factions or places a new player hasn't met yet (the Avgi, Hai, Saryd, Heliarch, Remnant, longcows and the Skadenga tours). The 15 that remain are generic spaceport chatter, jokes and early-game scenes that name none of them.
- **A guard against it happening again.** The app's tests now fail if a quote or its credit mentions one of those later-game names.

## Download

| System | File |
|---|---|
| Windows | `Endless.Sky.Mod.Manager_0.3.2_x64-setup.exe` (recommended), or the `.msi` |
| macOS | `Endless.Sky.Mod.Manager_0.3.2_aarch64.dmg` (Apple Silicon Macs only; no Intel build yet) |
| Linux | the `.AppImage`, `.deb` or `.rpm` |

Already have the app? It will offer this update itself. The other files (`.sig`, `.tar.gz`, `latest.json`) are for the built-in updater; you don't need them.

## Known caveats

- **Small screens.** On a screen shorter than 960 pixels (for example a 1366 × 768 laptop) the window may open taller than the screen. Drag it up or resize it; a later version will fit the window to the screen automatically.
- **Linux: multiple windows.** The single-copy safeguard from v0.3.0 is still in place; the underlying cause of the reported multiple windows is not confirmed. If you see it, please open an issue and attach the log (see the README for its location).
- **Unsigned installers.** Windows SmartScreen will warn ("More info → Run anyway"). On macOS, if the app is reported as "damaged" on first launch, run `xattr -cr "/Applications/Endless Sky Mod Manager.app"` once in Terminal, then open it. Code signing is planned.

## Feedback

Please open an issue, including the log (see the README for its location). Spoiler reports are welcome too.
