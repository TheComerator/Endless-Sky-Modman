# Endless Sky Mod Manager v0.3.1

A small, friendly update: the app now greets you with a proper start-up card.

## What's new

- **Start-up card.** When the app opens you see the logo, the version number, a line from Endless Sky's own story (with who said it) and a link to the MIT license. It stays until you click anywhere or press Esc. The new **About** button in the top bar brings it back, with a different quote each time.
  - The 30 quotes come from the game's own text and were picked to be funny or atmospheric without giving away the story.
  - Quotes are credited to Endless Sky, © Michael Zahniser and contributors (GPL-3+).
- **Bigger window.** The main window now opens larger (1320 × 960) and centred on the screen.

## Download

| System | File |
|---|---|
| Windows | `Endless.Sky.Mod.Manager_0.3.1_x64-setup.exe` (recommended), or the `.msi` |
| macOS | `Endless.Sky.Mod.Manager_0.3.1_aarch64.dmg` (Apple Silicon Macs only; no Intel build yet) |
| Linux | the `.AppImage`, `.deb` or `.rpm` |

Already have the app? It will offer this update itself. The other files (`.sig`, `.tar.gz`, `latest.json`) are for the built-in updater; you don't need them.

## Known caveats

- **Small screens.** On a screen shorter than 960 pixels (for example a 1366 × 768 laptop) the window may open taller than the screen. Drag it up or resize it; a later version will fit the window to the screen automatically.
- **Linux: multiple windows.** The single-copy safeguard from v0.3.0 is still in place; the underlying cause of the reported multiple windows is not confirmed. If you see it, please open an issue and attach the log (see the README for its location).
- **Unsigned installers.** Windows SmartScreen will warn ("More info → Run anyway"). On macOS, if the app is reported as "damaged" on first launch, run `xattr -cr "/Applications/Endless Sky Mod Manager.app"` once in Terminal, then open it. Code signing is planned.
- **Tested on:** Windows 11 and an Apple Silicon Mac. Linux builds are produced automatically but have had little hands-on testing.

## Feedback

Please open an issue, including the log (see the README for its location).
