# Prompt for a cloud Claude Code session: plugin load order

Copy everything below the line into a new cloud session that has the repository
`TheComerator/Endless-Sky-Modman` (public) attached.

---

You are working on **Endless Sky Mod Manager** (Tauri 2 + Rust + React/TypeScript), a desktop
plugin manager for the game *Endless Sky*. The owner (Jon) is not an experienced developer:
explain decisions in plain language in commit messages and your final report, and say
plainly when something is uncertain. Read `CLAUDE.md` at the repo root first (project rules,
design decisions A to L, and the roadmap section). Follow it. Also read `README.md`.

## Goal

Two deliverables, in this order.

### Deliverable 1: a feature request for the Endless Sky game (a document only)

Write `docs/proposals/endless-sky-load-order-request.md`. It will be posted by Jon to the
Endless Sky project himself (their RFC repository, `endless-sky/rfcs`, or an issue).
**Do not post, open issues or PRs anywhere outside this repository.** Write it as a proposal
a maintainer would want to read: short, concrete, respectful, no demands.

It must contain:

1. **Problem.** The game loads plugin folders in sorted order of folder name and a later
   plugin overrides fields an earlier one set. So the order is decided by a name chosen for
   other reasons (see `GameData::LoadSources` and `Files::ListDirectories` in the game source,
   which sorts with `std::sort` over paths; `UniverseObjects::LoadFile` for how a repeated
   definition is loaded onto the existing object). Plugin authors work around it with names
   like `zz-patch`, and uppercase sorts before lowercase.
2. **Evidence.** From a real set of 37 plugins (28 enabled), the manager's read-only scan
   found 88 overlaps (70 objects, 18 images) among 11 plugin pairs, e.g. one plugin
   overriding 17 definitions of another; the outcome depends on spelling and capitalisation
   of folder names. Use these numbers as they are and state they come from one real install.
3. **Proposal, kept small and backward compatible.** Recommend one primary option and describe
   one alternative:
   - *Primary:* an optional `order` block in `plugins.txt` (the file the game already reads
     and writes for enabled state), listing plugin names from first loaded to last loaded.
     Plugins not listed load after the listed ones in today's sorted order, so existing
     installs behave exactly as before. A missing block changes nothing.
   - *Alternative:* optional declarative hints in a plugin's own `plugin.txt` (e.g. under
     `dependencies`, "loads after" a named plugin), which expresses the author's intent and
     lets tools compute an order.
4. **Simple implementation steps** for the game (C++), naming the real functions: where the
   order would be read and stored (`PluginManager`), where `LoadSources` would sort the local
   plugin list by it instead of by path, and an optional Preferences > Plugins "move up /
   move down". Keep it to a short numbered list and note what is deliberately *not* proposed.
5. **Compatibility and risks**, **what the manager does meanwhile** (folder-name prefixes,
   removable), and **open questions** for the maintainers.

Verify every claim about the game against the game's source on GitHub
(`endless-sky/endless-sky`, `source/`) before writing it. Quote function names exactly.
If you cannot confirm something, say "unconfirmed" in the document.

### Deliverable 2: load order management in the manager, as a removable module

Build it so it can be switched off by the user, or deleted from the codebase, without
touching the rest of the app. This exists only until/unless the game supports load order
natively.

**How it works (decided).** The game sorts the folders in the user plugins directory by name.
The manager gives each *managed* plugin folder a fixed-width numeric prefix, `NNN-<original
name>` (e.g. `010-Cromha Expansion Plugin`), in steps of 10, and renames folders to apply a
user-chosen order. Symlinks and junctions were considered and **rejected** (identity problem
is unchanged, real folders would have to move out of `plugins/`, they fail inside Flatpak,
need privileges on Windows). Do not use them.

**Hard rules and facts you must respect**

- **Identity.** A plugin's identity is the `name` in its `plugin.txt`, else its folder name.
  Renaming a folder changes identity only for plugins *without* a `plugin.txt` name (7 of 37
  in the owner's set). In version 1, **leave those plugins out of ordering** and show why.
  **Do not write any file inside a plugin folder.** Writing a one-line `plugin.txt` for them
  is a possible follow-up that needs Jon's explicit approval; document it as an open option,
  do not implement it.
- **Records.** Install records (`records.rs`) key on the folder name (decision A). Store the
  original folder name and the current prefixed name; installs, updates and uninstalls must
  resolve a plugin through its record and keep its prefix. Profiles and `plugins.txt` use
  identities, not folder names, so they must keep working unchanged for named plugins.
- **Safety.** Renames only while the game is not running (reuse the existing game-running
  check). Write a small journal of intended renames *before* doing them, apply them, then
  clear it; on startup, finish or roll back an interrupted run. Never overwrite an existing
  folder. Back up `plugins.txt` as the existing code does. Refuse (with a clear message) on a
  name collision or an over-long Windows path. Only plugins the manager installed are
  renamed; unmanaged plugins are never touched.
- **Reversible.** One action restores every original folder name from the records. Turning
  the feature off must offer to do exactly that.
- **Sorting.** The game compares paths case-sensitively and unzipped folders load before
  `.zip` plugins; prefixed folders sort before unprefixed ones (digits before letters).
- **Dependencies.** If plugin A `requires` B, a suggested order places B first. Use the
  existing `plugin_meta`/`resolve` data. This is a suggestion the user can override.
- **Overlaps.** `crates/esmm-core/src/overlap.rs` already finds who wins each overlap in
  load order. Reuse it to show the effect of an order (before/after) and to explain *why* a
  plugin wins.

**Modularity requirements**

- A new, self-contained core module `crates/esmm-core/src/load_order.rs` (pure logic: compute
  an order, produce a rename plan, apply it with the journal, restore). Keep its public
  surface small and put the "how is order applied" part behind a trait (for example
  `LoadOrderBackend`) with the folder-prefix implementation as the only one, so a future
  native game mechanism can be added as another backend without touching callers.
- The rest of the code must call it only through that surface. Removing the feature must mean
  deleting the module, its shell commands and its UI view, and nothing else breaks. Add a
  short `docs`-style comment at the top of the module saying exactly which files to delete.
- A Settings switch **"Manage plugin load order"**, **off by default**. When off: no
  renames, no prefixes, no UI beyond the switch and a one-line explanation. Persist it in
  the existing settings file. Turning it off after use offers the restore from above.
- Also gate the UI view behind that setting. Consider a Cargo feature flag only if it is
  simple; the runtime switch is required, the compile-time one is optional.

**UI (keep it simple)**: a "Load order" view listing managed plugins in order, with move
up/down (drag is optional), a "Suggest order from dependencies" button, an "Apply" step that
shows a plan dialog of exactly which folders will be renamed (reuse the existing review
dialog pattern), a banner on any overlap whose winner changes, and "Restore original names".
Follow the style of the existing views and the project's plain-language wording.

## Process and guardrails

- Plan first: before writing code, post a numbered plan (steps, which files, what depends on
  what) in your first message, then proceed unless something in it is risky or unclear.
- Work on a branch named `load-order` and open a **pull request** into `main`. Do not push to
  `main`. Small, focused commits with a plain-language message ending with the repo's
  `Co-Authored-By` convention if you use one.
- **Do not** bump versions, create tags, edit `.github/workflows/release.yml`, touch the
  updater key or `tauri.conf.json` signing/updater settings, or publish anything.
- Add tests for every rule above: prefix computation and renumbering, collision and path-length
  refusal, journal recovery after a simulated interruption, restore, identity-safe skipping of
  plugins without a name, record handling across install/update/uninstall, and the settings
  switch off state. Use temp directories only; never touch a real plugins folder.
- Run, and make green: `cargo fmt --check`, `cargo clippy --all-targets`,
  `cargo test --workspace`, then in `app/`: `npm run build` and `npm test`. The repo's CI
  (`.github/workflows/ci.yml`) runs the same plus a check that `app/src/bindings` matches what
  the Rust code generates. If you add or change exported types, regenerate them by running
  `cargo test` and commit the result.
- Update `CLAUDE.md` (decision notes + roadmap) and the README feature list briefly. Do not
  rewrite unrelated parts.

## Final report (plain language, for a non-developer)

1. What you built and where (files).
2. How to try it, step by step, on a throwaway folder (not a real plugin set).
3. What you could not verify and what assumptions remain (game-source claims especially).
4. Anything you decided differently from this prompt, and why.
5. The open questions for Jon (at minimum: whether to add a minimal `plugin.txt` to
   plugins that lack a name so they can be ordered too).
