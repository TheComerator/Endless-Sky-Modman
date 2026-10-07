# Endless Sky Mod Manager v0.2.1

Small quality-of-life update, mostly around profiles.

## What's new

- **New empty profile.** On the Profiles page, type a name and click **New empty profile** for a vanilla run: switching to it turns every plugin off, and nothing is uninstalled. Switch back to another profile and your plugins return instantly.
- **Clearer "Apply Profile" button.** The old "Re-apply" button is now **Apply Profile**, with an explanation when you hover it. It only appears on the active profile when your plugins were changed outside the manager (for example in the game's own Plugins screen), so there is nothing to click when nothing needs applying. The warning banner's button uses the same wording.
- **A warning banner you can't miss.** The "your plugins changed outside the manager" and "Endless Sky is running" banners are brighter, with a slow breathing amber glow. If you've turned animations off in Windows, you get the same glow without the movement.
- Behind the scenes: the project now runs its test suite automatically on Windows and Linux for every change.

## Download

| System | File |
|---|---|
| Windows | `Endless.Sky.Mod.Manager_0.2.1_x64-setup.exe` (recommended), or the `.msi` |
| macOS | `Endless.Sky.Mod.Manager_0.2.1_aarch64.dmg` (Apple Silicon Macs only; no Intel build yet) |
| Linux | the `.AppImage`, `.deb` or `.rpm` |

Already have the app? It will offer this update itself. The other files (`.sig`, `.tar.gz`, `latest.json`) are for the built-in updater; you don't need them.

## Known caveats

- **Unsigned installers.** Windows SmartScreen will warn ("More info → Run anyway"). On macOS, if the app is reported as "damaged" on first launch, run `xattr -cr "/Applications/Endless Sky Mod Manager.app"` once in Terminal, then open it. Code signing is planned.
- **Profiles don't record versions.** Someone importing your profile gets the current version of each plugin from the official catalog.
- **Tested on:** Windows 11 and an Apple Silicon Mac, both with Steam installs of Endless Sky 0.11. Linux builds are produced automatically but have had little hands-on testing, so reports from Linux are especially welcome.

## Feedback

Please open an issue, including the log (see the README for its location).
