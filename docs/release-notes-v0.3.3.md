# Endless Sky Mod Manager v0.3.3

Finishing the start-up card's spoiler clean-up that v0.3.2 started.

## What's changed

- **Six more spoilers removed.** v0.3.2 removed the quotes that *named* later-game species and places. This time every remaining quote was also checked for *where* it appears in the game, and six turned out to come from later content: the Free Worlds campaign, a later Syndicate storyline, the Bloodsea story events and files for regions outside human space. They're gone.
- **Back to 30 quotes.** 21 new lines were added, all from places any new player can find in human space: spaceport news chatter, hails from passing ships, the intro missions and ship and outfit descriptions. Each was checked word for word against the game's own files.
- **A stronger guard.** The app's tests now also block "Bloodsea" and "Bloom", and fail if a quote is too long for the card.

## Download

| System | File |
|---|---|
| Windows | `Endless.Sky.Mod.Manager_0.3.3_x64-setup.exe` (recommended), or the `.msi` |
| macOS | `Endless.Sky.Mod.Manager_0.3.3_aarch64.dmg` (Apple Silicon Macs only; no Intel build yet) |
| Linux | the `.AppImage`, `.deb` or `.rpm` |

Already have the app? It will offer this update itself. The other files (`.sig`, `.tar.gz`, `latest.json`) are for the built-in updater; you don't need them.

## Known caveats

- **Small screens.** On a screen shorter than 960 pixels (for example a 1366 × 768 laptop) the window may open taller than the screen. Drag it up or resize it; a later version will fit the window to the screen automatically.
- **Linux: multiple windows.** The single-copy safeguard from v0.3.0 is still in place; the underlying cause of the reported multiple windows is not confirmed. If you see it, please open an issue and attach the log (see the README for its location).
- **Unsigned installers.** Windows SmartScreen will warn ("More info → Run anyway"). On macOS, if the app is reported as "damaged" on first launch, run `xattr -cr "/Applications/Endless Sky Mod Manager.app"` once in Terminal, then open it. Code signing is planned.

## Feedback

Please open an issue, including the log (see the README for its location). Spoiler reports are welcome too.
