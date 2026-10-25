- Feature Name: `plugin_load_order`
- Affected audience: Players, Plugin Authors, Game/Tool Developers
- RFC PR: *(to be filled in if this is turned into a pull request)*
- Relevant Issues/RFCs: none found yet

> **Status: draft, for discussion.** Written from the point of view of the maintainer of an
> unofficial plugin manager ([Endless Sky Mod Manager](https://github.com/TheComerator/Endless-Sky-Modman)).
> Everything said about the game was checked against the `endless-sky/endless-sky` source on
> `master` (commit `57c62bb`, 2026-10-10). Where something could not be confirmed, it says
> **unconfirmed**. Function names are quoted exactly; line numbers are left out on purpose.

# Summary

Today the order in which plugins are loaded is the sorted order of their **folder names**, and a
plugin loaded later wins wherever two plugins set the same field of the same object. Players and
plugin authors have no direct way to choose that order. This RFC proposes a small, optional,
backward-compatible way to state it: an `order` block in the `plugins.txt` file the game already
reads and writes. A second option (author-supplied "loads after" hints in `plugin.txt`) is
described as an alternative.

# Motivation

## The problem

What the source does (confirmed on `master`; older releases were not checked):

1. `GameData::LoadSources` builds the list of data sources. It starts with `Files::Resources()`,
   then the global plugins, then the user plugins. For each of those it first loads **unzipped**
   plugin folders, taken from `Files::ListDirectories(...)`, and only afterwards the `.zip`
   plugins, taken from `Files::List(...)`. A folder only counts if it passes
   `PluginManager::IsPlugin`. Each one goes through `PluginManager::Load` (which returns nothing
   for a plugin whose dependency lists are invalid); if the plugin is enabled, its path is
   appended to `sources`.
2. `Files::ListDirectories` collects the directories and calls `sort(list.begin(), list.end())`
   on a `std::vector<std::filesystem::path>`. So the order is the library's ordering of the
   paths, i.e. of the folder names.
3. `UniverseObjects::Load` walks `sources` front to back, gathers `Files::RecursiveList(source /
   "data")` for each, and passes the files to `UniverseObjects::LoadFile` in that sequence.
   (The comment inside that loop says the iteration starts with the *last* directory, so that
   folders near the start can override later ones; the loop as written goes front to back, and
   the base game, which comes first, is overridden by plugins. I read the code as authoritative
   and the comment as out of date, but that is an interpretation.)
4. What happens when an object is defined again depends on its kind. For most of the kinds I
   looked at (`ship`, `outfit`, `planet`, `system`, ...) `UniverseObjects::LoadFile` does not
   replace the object: it fetches the existing one (for example `ships.Get(name)`) and calls its
   `Load(node, ...)` on it, so fields that are set again take the new value and fields that are
   not mentioned keep the old one. Only a preceding `overwrite` node resets the object first
   (`if(overwrite) *ship = Ship();`). `Ship::Load` even notes that the attribute list is left
   alone "so that it is permissible to override one ship definition with another".
   **Other kinds behave differently**, and I only spot-checked these: `Mission::Load` and
   `FormationPattern::Load` refuse a second definition of the same name ("Duplicate definition
   ..."; for missions the comment says the class "is not designed to allow merging or overriding"),
   so there the *earlier* plugin wins; `Conversation::Load` starts with `nodes.clear()`, so it is
   always replaced; `Phrase::Load` appends to what is there. I did not check every kind, so the
   rule in the next paragraph is exact for field-by-field kinds and **unconfirmed** for the rest.
5. Images follow the same direction: `GameData::FindImages` walks `sources` in order and
   `ImageSet::Add` stores each file in `framePaths[...][frameNumber]`, so a later source's file
   for the same image and frame replaces an earlier one (per variant: a later plugin that ships
   only the normal-size frame leaves an earlier plugin's `@2x` frame in place).
6. Two plugins with the same name cannot both load: `PluginManager::Load` keeps the first and
   skips the later one with a warning. "First" follows the order above, so it is decided by the
   sorted folder names within each group (global unzipped, global zip, user unzipped, user zip).

So for the field-by-field kinds the effective rule is: **the plugin whose folder name sorts last
has the final say on any field two plugins both set.** The folder name is chosen by whoever
packaged the plugin, usually for reasons that have nothing to do with load order. Authors work
around this with names such as `zz-patch`, and players rename folders by hand. Because paths
compare character by character, capitalisation matters: on Linux with libstdc++ I sorted `Zeta`,
`alpha`, `Alpha`, `zz-patch`, `010-Foo`, `-dash` and `_under` with the same `std::sort` over
`std::filesystem::path` and got

```
-dash   010-Foo   Alpha   Zeta   _under   alpha   zz-patch
```

(`-` and other characters below `0` first, then digits, uppercase, `_`, lowercase).
That is the standard library's behaviour as I observed it, not something written in the game's
source. **Unconfirmed on Windows and macOS**, where the native path type and the library differ.

## Evidence

One real install, measured by the manager's read-only overlap scan (it reads each plugin's
`data/` text and lists the names of its images and sounds; nothing was changed):

| | |
|---|---|
| Plugins installed | 37 (28 enabled) |
| Overlaps found among enabled plugins | 88 (70 objects such as ships, outfits and planets, 18 images, 0 sounds) |
| Plugin pairs involved | 11 |
| Largest single case | one plugin overrides 17 definitions of another |

These numbers come from **one person's install**, so they show that the situation is real, not
how common it is. An "overlap" here means a later plugin sets a field that an earlier one also
touched; some of these are surely intentional (a patch plugin is supposed to win), the scan
treats list-style fields such as a system's `link` conservatively so a few may be harmless, and it
applies the field-by-field rule to every kind, which item 4 above shows is not right for all of
them (missions, for example). The
point is that for every one of them the winner is decided by how two folder names happen to
sort, and nobody can change it without renaming a folder.

# Detailed Design

## Primary proposal: an `order` block in `plugins.txt`

`plugins.txt` already holds the per-user plugin state and is already read and written by the
game. Add an optional second top-level node next to `state`:

```
state
	"Some Plugin" 1
	"Another Plugin" 1
order
	"Another Plugin"
	"Some Plugin"
```

* `order` lists plugin **names** (the same names `state` uses: `name` from `plugin.txt`, else the
  folder name), from **first loaded to last loaded**. The last one listed wins.
* Plugins that are not listed load **after** the listed ones, in today's sorted order.
* A missing or empty `order` block changes nothing, so every existing install behaves exactly as
  it does now.
* Scope: the unzipped plugins in the user plugins folder. Global plugins and `.zip` plugins keep
  today's behaviour. (Open question below.)
* Which file: `PluginManager::LoadSettings` reads two `plugins.txt` files, the one in
  `Files::Resources()` and then the one in `Files::Config()`. The simplest rule is that `order`
  is only read from the second (per-user) one, which is also the one `PluginManager::Save`
  writes.

## Implementation sketch (C++)

1. **Read and store the order in `PluginManager`.** Extend `LoadSettingsFromFile` (in
   `PluginManager.cpp`), which today only looks at `state` nodes and skips every other top-level
   node, to also read an `order` node (from the per-user file only, see above) into a `std::vector<std::string>` kept next to the
   `plugins` set. Add `static const std::vector<std::string> &Order();` and a setter to
   `PluginManager.h`.
2. **Write it back in `PluginManager::Save`.** `Save` rewrites `plugins.txt` from memory and is
   called from `main()` when the game quits normally (not on a crash, a startup error or the
   console-only modes), so without this step the game would erase the
   block the next time it quits. Write the `order` node after `state` when the list is not empty.
3. **Apply it in `GameData::LoadSources`.** The plugin's name is only known after
   `PluginManager::Load` has run, so for the user plugins: load each folder as today (this keeps
   the "first one with a name wins" rule on path order), but collect the enabled ones in a local
   list instead of appending to `sources` straight away; then stable-sort that list by position
   in `PluginManager::Order()` (unlisted plugins after all listed ones, keeping path order among
   themselves) and append it to `sources`. Nothing else in `UniverseObjects::Load` or
   `FindImages` needs to change, because both already follow `sources`.
4. **Optional, can be a later change: Preferences > Plugins "move up / move down".**
   `PreferencesPanel::DrawPlugins` iterates `PluginManager::Get()`, which is a `Set<Plugin>`
   backed by a `std::map<std::string, Plugin>`, so the list on screen is ordered by name (byte order, so
   uppercase before lowercase), not in load order. Showing and editing the order there would
   mean sorting that list by `Order()`, adding two buttons to the `plugins` interface and a
   `PluginManager::MovePlugin(name, delta)`. I did not check how much work the interface
   definition needs: **unconfirmed**.
5. **Tests.** Unit tests for the order resolution (listed before unlisted, stable among
   unlisted, unknown names ignored, missing block unchanged) in `tests/unit`.

**Deliberately not proposed:** changing the default sorted order; automatic ordering from
dependencies inside the game; any UI for resolving individual object clashes; ordering global or
`.zip` plugins; changing the "first plugin with a name wins" rule.

## Alternative: "loads after" hints in `plugin.txt`

Let a plugin say, under `dependencies`, which plugins it wants to load after:

```
dependencies
	"loads after"
		"Some Other Plugin"
```

`Plugin::PluginDependencies` already has the `required`, `optional` and `conflicted` sets and
`PluginManager::Load` already parses them; a `loadsAfter` set would sit beside them, and
`LoadSources` would apply the hints as a stable topological sort of the path order (ignoring
names that are not installed, and reporting cycles).

* For: the author's intent travels with the plugin, nobody has to maintain a list, and tools can
  compute an order from the data.
* Against: it cannot express a player's own preference, only an author's; authors have to know
  about other plugins; two plugins that both want to be last still need a tie-breaker.
* Cost on old versions: `PluginManager::Load` prints "Skipping unrecognized attribute" for an
  unknown key under `dependencies`, so a plugin using the new key would log a warning on older
  game versions (confirmed in `master`; older releases not checked; harmless otherwise).

The two are not exclusive: hints could produce the default order and the `order` block could
override it.

# Drawbacks

* **Older game versions drop the block.** In `master`, reading ignores a top-level node other
  than `state` and `PluginManager::Save` rewrites the file without it at the next normal exit; I
  could not check older releases, so for those this is an inference. Someone switching between
  versions would lose the order, not break anything.
* **Names can change.** If a plugin changes its `name`, it falls out of the list and loads last
  until re-added.
* **One more thing to explain** to players, and a new way to get a surprising load order if the
  list is edited by hand. Showing it in Preferences (step 4) would help.
* Unlisted plugins always load after listed ones, so a newly installed plugin silently becomes
  the winner over everything listed. That is the simplest rule, but it may not be the best one.

# Alternatives

* **Do nothing; keep naming folders.** This is what everyone does today. Its cost is the problem
  described above: the order is coupled to folder names, and a tool or a player has to rename
  folders (and, for plugins without a `name` in `plugin.txt`, renaming a folder also changes the
  plugin's identity in `plugins.txt` and in other plugins' dependencies).
* **"Loads after" hints** (above).
* **A separate order file** instead of `plugins.txt`. Possible, but `plugins.txt` is already the
  per-user plugin file the game reads at start and writes at exit.
* **Symlinks or junctions with chosen names.** Considered and rejected by the manager: the game
  would still identify the plugin by the link's name, the real folders would have to live
  outside `plugins/`, links do not work inside a Flatpak sandbox, and Windows needs extra
  privileges.

# What the mod manager does in the meantime

[Endless Sky Mod Manager](https://github.com/TheComerator/Endless-Sky-Modman) is adding an
**opt-in, off by default** "Manage plugin load order" feature that works within today's rules:
it gives plugin folders a fixed-width numeric prefix (`010-Name`, `020-Name`, ...), which sorts
before letters, and renames folders to apply the order the user chose. It only touches plugins it
installed itself and that have a `name` in their `plugin.txt` (so renaming the folder does not
change their identity), records the original names so a single action restores them, and keeps a
journal so an interrupted rename is finished or undone at the next start. It is designed as a
removable module: if the game gains a native mechanism like the one above, it can be retired.

# Unresolved Questions

1. Is `plugins.txt` the right home for the order, or would maintainers prefer a separate file or
   the `plugin.txt` hints?
2. Should unlisted plugins load after the listed ones (as proposed) or before them?
3. Should `order` also cover global plugins and `.zip` plugins, and if so, may it interleave
   zipped and unzipped ones? (Today every unzipped plugin loads before every zip.)
4. Should names of plugins that are no longer installed be kept in the list, so a plugin that is
   reinstalled gets its old place back? (Today `Save` writes only plugins that exist, so `state`
   entries for uninstalled plugins disappear at the next exit; that would be the precedent.)
5. Is the comment at the top of the loop in `UniverseObjects::Load` meant to describe something
   other than what the code does? If not, it could be corrected as part of this work.
6. Does path sorting behave the same way on Windows and macOS as on the Linux library I tested?
   (Unconfirmed; with an explicit `order` it would matter only for the unlisted plugins.)
