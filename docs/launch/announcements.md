# Announcement drafts

Edit freely. Replace anything in [brackets]. Post after a clean-machine install test and after screenshots are in the README. Check each community's own rules about tool announcements first.

Repo: https://github.com/TheComerator/Endless-Sky-Modman
Release: https://github.com/TheComerator/Endless-Sky-Modman/releases/latest

---

## Discord (short, for a plugins / tools channel)

**Endless Sky Mod Manager v0.1: install and switch plugins without unzipping anything**

I built a small desktop mod manager for Endless Sky, in the spirit of r2modman. Browse the official plugin catalog, one-click install (required plugins come along), update everything at once, and keep profiles for different playthroughs. It checks dependencies and conflicts before changing anything, and won't touch your files while the game is running.

It's an early release, tested on Windows + Steam, with Linux/macOS builds that need testers. Installers are unsigned for now, so Windows will show a SmartScreen warning.

[screenshot]

Download + details: https://github.com/TheComerator/Endless-Sky-Modman
I'd really like bug reports and ideas, especially from Linux/macOS users. It's MIT licensed.

---

## Reddit (r/EndlessSky)

**Title:** I made a mod manager for Endless Sky plugins (profiles, dependency checks, one-click updates)

**Body:**
I've been wanting something like r2modman for Endless Sky, so I made one. It reads the same official plugin catalog as the plugins page and handles the install/enable/update work for you.

What it does:
- Browse, search and install plugins, with required plugins installed automatically
- Update one plugin or all of them; new dependencies are handled
- Profiles, so a vanilla run and a heavy-mods run don't fight each other
- Explains conflicts and missing requirements in plain language *before* changing anything, and can disable the conflicting plugin for you
- Detects Steam (including Flatpak Steam), standalone and AppImage installs, with a Launch button

It's not a game launcher (ESLauncher2 does that well and the two work fine together); it focuses only on plugins.

Honest status: v0.1. Works on my Windows + Steam setup and has a decent automated test suite, but it hasn't been used by many people. The installers aren't code-signed yet, so expect a Windows SmartScreen warning ("More info → Run anyway"). Source is public if you'd rather build it yourself.

[screenshots]

Link: https://github.com/TheComerator/Endless-Sky-Modman
Feedback, bug reports and feature ideas are very welcome. Linux and Mac testers especially.

---

## Official forum / GitHub Discussions (longer, for developers and plugin authors)

**Endless Sky Mod Manager: a plugin-focused manager built on the official catalog**

Hello all. I've released v0.1 of a desktop manager for plugins. A few notes that may matter to developers and plugin authors in particular:

- **It uses the official index** (`endless-sky-plugins`' `plugins.json`) with ETag caching, and respects its `autoupdate` data for update checks.
- **It reads each plugin's `plugin.txt`** for `requires`, `conflicts`, `optional` and `game version`, and enforces them, since the game itself doesn't. It treats `game version` as a minimum, which is our own reading of an undocumented field; if that's wrong, I'd like to know.
- **It writes `plugins.txt` the way the game does** (`1`/`0`, same quoting), backs it up first, and refuses to write while the game is running.
- **Plugin identity** is the `plugin.txt` name, falling back to the folder name, matched against catalog names in a deliberately conservative way (never fuzzy).

For plugin authors: if your plugin's dependencies show up wrongly, that's a bug on my side and I'd like to fix it. Declaring `requires` / `conflicts` correctly in `plugin.txt` is what makes the checks useful.

It's MIT licensed and the repo documents how it works in detail (see CLAUDE.md). Link: https://github.com/TheComerator/Endless-Sky-Modman

---

## Friendly note for the ESLauncher2 community

Hi, I made a plugin-only manager that complements ESLauncher2 rather than replacing it: it doesn't manage game builds at all, just plugins (dependency and conflict checks, profiles). Both read the same plugin folder, so they can be used together. If you see anything that would make them play badly together, please tell me. [link]

---

## Plugin author outreach (optional, individual messages)

If you maintain a plugin with dependencies, a quick message works well: "I built a manager that enforces `plugin.txt` dependencies. Could you check how your plugin shows up?" It finds real bugs and makes authors allies.
