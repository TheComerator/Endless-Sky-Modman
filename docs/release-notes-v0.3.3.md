# Endless Sky Mod Manager v0.3.3

A spoiler clean-up for the start-up card's quotes, thanks to a player who pointed out that some of them gave away later parts of the game.

## What's changed

- **Spoiler-free quotes.** Every quote on the start-up card was checked again, both for what it says and for where it appears in the game. Lines that name species, factions or places a new player hasn't met yet were removed, along with lines that only show up in a later campaign, a faction's own storyline or after a story event. The card now has 30 quotes from places any new player can find in human space: spaceport news chatter, hails from passing ships, the intro missions and ship and outfit descriptions.
- **A guard against it happening again.** The app's tests now fail if a quote or its credit mentions one of a list of later-game names, or if a quote is too long for the card.

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
