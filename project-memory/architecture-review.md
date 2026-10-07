# Architecture review checklist for Jon

**Answered by Jon 2026-10-07:** 1 keep, 2 keep, 3 keep, 4 keep, 5 keep 14 days (a Settings log-level switch can wait until detailed logs are needed from users), 6 leave as is (no Linux machine; ask for a Flatpak Steam tester after launch), 7 manual click-through still to do.

The only roadmap item that needs a person rather than more code. These are the design
calls made by inference (no one had confirmed them) in decisions K, L and the game-install
work. Each has what was built, why, and the alternative. Reply with ✅ keep or ✏️ change;
anything marked change is a small, well-isolated edit.

Nothing here is a bug. All of it works and is tested; the question is only whether it's
what you'd have chosen.

## 1. Where the manager keeps its per-game state
**Built:** `records.json` and `profiles.json` live in `<app-data>/installs/<hash of the game's config folder>/`, so a native install and a Flatpak install each get their own records and profiles, while two installs that share a config folder (e.g. native + Steam) share them, the same way they share `plugins/`.
**Why:** the state describes a specific `plugins/` folder, so it should follow that folder.
**Alternative:** one global set of records/profiles regardless of install (simpler, but a profile made for one install would reference plugins another doesn't have).

## 2. Which actions ask first, and which just happen
**Built:** install, update, uninstall and profile switches always show the review dialog. An enable/disable toggle with no problems commits instantly with no dialog; if it has any issue (or an uninstalled optional dependency) it opens the dialog.
**Why:** toggling is frequent and reversible; the others change files or whole setups.
**Alternative:** confirm everything, or let the user choose "don't ask again".

## 3. Switching profiles is a checked, reviewable plan
**Built:** switching runs the same conflict/requirement checks as any change, so a profile that would enable conflicting plugins is blocked with the same override option (and, as of tonight, a "disable the other one instead" shortcut).
**Why:** a profile switch is the easiest way to silently create a broken setup.
**Alternative:** switch instantly and just warn afterwards.

## 4. Plugins that were already in the folder (not installed by the manager)
**Built:** if the catalog has one clear match, it's adopted automatically and silently (a one-line toast). With several possible matches you get a "Link to…" picker on the Installed page; with none it just shows an "Unmanaged" badge and stays fully usable. Uninstalling one works but warns that the manager didn't install it. Nothing unmanaged is ever deleted silently.
**Alternative:** always ask before adopting; or never adopt and keep unmanaged plugins entirely separate.

## 5. Logging
**Built:** one daily-rolling log file in the OS's standard app-log folder, covering plan/commit steps, downloads, catalog fetches, game launches and every UI command (not per-byte download progress or the 4-second game poll's detail). Files older than **14 days** are deleted at startup (added tonight; this used to be "never pruned").
**Questions:** is 14 days the right retention? Should the log level be adjustable from Settings (today only via an environment variable)?

## 6. Steam running as a Flatpak (Linux)
**Built:** its library is now found at the path Steam's own Flatpak manifest sets (confirmed from the real manifest). One assumption remains unverified: that a native game launched by Flatpak-Steam writes its saves to the normal home folder rather than inside Steam's sandbox, so Endless Sky's config is looked for in the usual place. It matches how most people report it working, but I couldn't confirm it from source.
**Needs:** someone with Flatpak Steam to launch the game once and confirm where `plugins.txt` ends up. If it's elsewhere, add that folder as a custom install (Settings) and tell me the path.

## 7. Two things worth a quick manual look when you're next at the app
These are logic I verified by tests and code review but couldn't click through myself:
- **Fix buttons resume the original action** (e.g. install something whose dependency needs "Install X first": after X installs, the first dialog should reappear).
- **"Disable the other one instead"** on an enable or profile-switch conflict.
