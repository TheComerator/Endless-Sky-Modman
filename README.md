# Endless Sky Mod Manager

![Status](https://img.shields.io/badge/status-v1%20feature--complete-4ADE9C)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-7AA2FF)
![License](https://img.shields.io/badge/license-MIT-C792EA)

A desktop mod manager for *Endless Sky*, the open-source 2D space trading/exploration/combat game — modeled on r2modman. Browses the official community plugin catalog, installs/updates/removes plugins without manual zip-wrangling.

See `CLAUDE.md` for project standards, the confirmed plugin catalog schema, and current status. See `project-memory/` for lessons learned and patterns discovered as the project develops.

## Roadmap

**v1 is feature-complete**, verified end-to-end on Windows — Steam detection, catalog browse, plugin install, and launch, confirmed on a real install on 2026-10-01.

### ✅ Shipped

- [x] Browse & search the live plugin catalog
- [x] Install plugins with recursive dependency resolution
- [x] Enable / disable and uninstall plugins
- [x] Update checking — one at a time, or "update all" in a batch
- [x] Profiles — switch, create, rename, delete, drift detection
- [x] Game install detection — Steam, native, and custom paths
- [x] Launch-game button
- [x] File-based logging for after-the-fact debugging
- [x] Cross-platform CI — automated Windows, macOS & Linux builds
- [x] Windows build & test environment verified (136 core tests, clippy, fmt)
- [x] First real-world run verified — Steam detect → browse → install → launch, plugin confirmed active in-game
- [x] App icon — ship-with-engine-trail logo replacing the default Tauri placeholder
- [x] Manual UI pass (profile rename, enable/disable toggle, logging) verified against the real Windows app and its log file — found and fixed a missing "all mods are up to date" confirmation
- [x] Plugin license shown as a badge in the catalog browser
- [x] Log files older than 14 days are pruned automatically on startup
- [x] Missing-after-profile-switch plugins shown as a real list with a "Download all" button, replacing a button pile that could grow unbounded

### 🔜 Remaining

- [ ] Review the inferred Tauri shell & logging architecture decisions

### ⚠️ Known gaps (not v1 blockers)

- Conflicts outside install plans can only be overridden, not auto-resolved
- A "fix" suggestion starts a new plan; the original action must be re-run after
- An ambiguous unmanaged-plugin match could create a duplicate folder

### ℹ️ Known limitations (by design)

- A stalled download can't be cancelled at the network layer directly
- Steam-as-Flatpak isn't auto-detected; AppImage installs are added manually
- Updating a plugin with a brand-new dependency reports it but won't auto-install it

Full detail, including the reasoning behind each decision: [`CLAUDE.md`](CLAUDE.md#current-status).
