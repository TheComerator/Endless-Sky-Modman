# Announcement drafts

Edit freely and swap anything in [brackets]. Before posting: add real screenshots, read each community's rules about sharing tools, and make sure the latest release (v0.2.0 or newer) is published.

Repo: https://github.com/TheComerator/Endless-Sky-Modman
Downloads: https://github.com/TheComerator/Endless-Sky-Modman/releases/latest

---

## Discord (short, for a plugins / tools channel)

**Made a plugin manager for Endless Sky 👋**

Hey all! I got tired of unzipping plugins by hand, so I made a little desktop app for it, kind of like r2modman. You browse the official plugin catalog, click install, and it grabs whatever the plugin needs too. You can also keep profiles for different playthroughs and share them as a file with friends.

It checks for conflicts and missing requirements before it touches anything, and it won't mess with your files while the game's running. It also updates itself when there's a new version.

It's pretty new, so it's only had real-world testing on Windows and Apple Silicon Macs with Steam. Linux builds exist but I'd love someone to try them. The installers aren't code-signed yet, so Windows will throw a SmartScreen warning at you (More info → Run anyway).

[screenshot]

Grab it here: https://github.com/TheComerator/Endless-Sky-Modman/releases/latest
Bugs, ideas, "this is confusing" comments: all welcome. Free and open source (MIT).

---

## Reddit (r/EndlessSky)

**Title:** I made a plugin manager for Endless Sky (one-click installs, profiles you can share, conflict checks)

**Body:**
Hi everyone! I wanted something like r2modman for Endless Sky, so I built one. It uses the same official plugin catalog as the plugins page and does the boring install/enable/update stuff for you.

What it does:
- Browse, search and install plugins. Anything a plugin requires gets installed along with it
- Update one plugin or everything at once
- Profiles, so your vanilla run and your everything-turned-on run don't step on each other
- **Share a profile as a file.** Export your setup, send it to a friend, they import it and the app offers to download whatever they're missing
- Tells you about conflicts and missing requirements in plain English *before* changing anything, and can just turn off the conflicting plugin for you
- Finds your game (Steam, Flatpak Steam, standalone, AppImage) and has a Launch button
- Updates itself, but only when you click the button. You can switch the check off in Settings

It's not a game launcher. ESLauncher2 does that job well and the two get along fine, since this only deals with plugins.

Being honest about where it's at: it's an early version. I've used it a lot on Windows + Steam and on an Apple Silicon Mac, and it has a decent set of automated tests, but not many people have used it yet. Linux builds exist and are barely tested, and there's no Intel Mac build yet. The installers aren't code-signed, so Windows shows a SmartScreen warning (More info → Run anyway), and on a Mac you may need one Terminal command the first time, which is in the README. The source is public if you'd rather build it yourself.

[screenshots]

Link: https://github.com/TheComerator/Endless-Sky-Modman
If something breaks or feels off, please tell me. Linux and Intel Mac folks especially, I'd love to hear how it goes.

---

## Official forum / GitHub Discussions (a bit longer, for developers and plugin authors)

**Endless Sky Mod Manager: a plugin manager built on the official catalog**

Hello! I've put out a desktop manager for plugins, and a few details might interest developers and plugin authors:

- **It uses the official index** (`endless-sky-plugins`' `plugins.json`), cached with ETags, and relies on its `autoupdate` data so update checks need no API calls.
- **It reads each plugin's `plugin.txt`** for `requires`, `conflicts`, `optional` and `game version`, and enforces them, since the game doesn't. I treat `game version` as a *minimum*, which is just my best guess about an undocumented field. If that's wrong, please tell me.
- **It writes `plugins.txt` the way the game does** (`1`/`0`, same quoting), backs it up first, and refuses to write while the game is running.
- **Plugin identity** is the `plugin.txt` name, falling back to the folder name, and it matches that against catalog names very conservatively (never fuzzy).
- **Profiles are shareable files.** They only list plugin names (no versions, no URLs), and anything imported is looked up in the official catalog, so a shared file can't point the app at a random download.

If you're a plugin author and your dependencies show up wrong, that's a bug on my side and I want to fix it. Declaring `requires` and `conflicts` properly in `plugin.txt` is what makes the checks useful.

It's MIT licensed, and the repo explains how everything works in detail (see CLAUDE.md). Link: https://github.com/TheComerator/Endless-Sky-Modman

---

## Friendly note for the ESLauncher2 community

Hi! I made a plugin-only manager that's meant to sit alongside ESLauncher2, not replace it. It doesn't touch game builds at all, just plugins (dependency and conflict checks, profiles). Both use the same plugin folder, so they should coexist happily. If you spot anything that makes them clash, please let me know. [link]

---

## Plugin author outreach (optional, one message each)

If you maintain a plugin that has dependencies, something short and casual works: "Hey! I built a manager that enforces `plugin.txt` dependencies. Would you mind seeing how your plugin shows up in it?" It turns up real bugs and usually makes friends.

---

## Replies to keep handy

- **"Is it safe?"** It backs up `plugins.txt` before every change, won't write while the game's running, shows you every change before it happens, and the only thing it contacts besides the plugin catalog is GitHub (to check for app updates, which you can switch off).
- **"Why a SmartScreen warning?"** The installers aren't code-signed yet (that costs money or an application). "More info → Run anyway" is normal for this stage, and the source is public if you'd rather build it.
- **"Mac says it's damaged."** It isn't, that's just macOS being strict about unsigned apps. Run `xattr -cr "/Applications/Endless Sky Mod Manager.app"` in Terminal once and open it again.
- **"Does it work on Intel Macs / Linux?"** Not on Intel Macs yet (Apple Silicon only). Linux builds exist but are barely tested, so reports are very welcome.
