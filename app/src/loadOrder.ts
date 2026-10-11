// Pure helpers for the optional "Load order" page (delete with the feature; see
// crates/esmm-core/src/load_order.rs for the full list of files).
import type { LoadOrderDependencyNote } from "./bindings/LoadOrderDependencyNote";
import type { LoadOrderExcludeReason } from "./bindings/LoadOrderExcludeReason";
import type { LoadOrderWinnerChange } from "./bindings/LoadOrderWinnerChange";

/** A copy of `list` with the item at `index` moved one place up (-1) or down (+1). */
export function moveItem<T>(list: readonly T[], index: number, delta: -1 | 1): T[] {
  const target = index + delta;
  if (index < 0 || index >= list.length || target < 0 || target >= list.length) return [...list];
  const next = [...list];
  [next[index], next[target]] = [next[target], next[index]];
  return next;
}

/** A copy of `list` with the item at `index` moved to the very top or the very bottom. */
export function moveToEnd<T>(list: readonly T[], index: number, end: "top" | "bottom"): T[] {
  if (index < 0 || index >= list.length) return [...list];
  const rest = list.filter((_, i) => i !== index);
  return end === "top" ? [list[index], ...rest] : [...rest, list[index]];
}

export function sameOrder(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((item, i) => item === b[i]);
}

/** "010" for a folder prefix number, "none" for a plugin that has no number yet. */
export function formatNumber(n: number | null): string {
  return n === null ? "none" : String(n).padStart(3, "0");
}

/** True when `a` and `b` hold the same items, in any order. */
export function samePlugins(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && new Set(a).size === a.length && b.every((item) => a.includes(item));
}

/**
 * The order to show: the user's unsaved arrangement (`draft`) while it still covers exactly the
 * plugins that are installed, otherwise the order saved on disk. A draft that no longer fits
 * (a plugin was added or removed meanwhile) is dropped rather than shown wrong.
 */
export function effectiveOrder(draft: readonly string[] | null, saved: readonly string[]): readonly string[] {
  return draft && samePlugins(draft, saved) ? draft : saved;
}

/** "1 requirement loads first" / "3 requirements load first". */
export function describeSuggestion(fixed: number): string {
  return `Moved plugins so ${fixed} requirement${fixed === 1 ? " loads" : "s load"} first. Nothing changes until you apply.`;
}

export interface WinnerGroup {
  /** Who would win after the change (null: nobody would override anyone any more). */
  now: string | null;
  /** Who wins today (null: there is no override today). */
  was: string | null;
  count: number;
  examples: LoadOrderWinnerChange[];
}

/** Overlap changes grouped by (new winner, old winner), biggest group first. */
export function groupWinnerChanges(changes: readonly LoadOrderWinnerChange[]): WinnerGroup[] {
  const groups = new Map<string, WinnerGroup>();
  for (const change of changes) {
    const key = `${change.now ?? ""}\u0000${change.was ?? ""}`;
    const group = groups.get(key) ?? { now: change.now, was: change.was, count: 0, examples: [] };
    group.count += 1;
    group.examples.push(change);
    groups.set(key, group);
  }
  return [...groups.values()].sort(
    (a, b) => b.count - a.count || (a.now ?? "").localeCompare(b.now ?? "") || (a.was ?? "").localeCompare(b.was ?? ""),
  );
}

export function describeGroup(group: WinnerGroup): string {
  const places = `${group.count} place${group.count === 1 ? "" : "s"}`;
  if (group.now && group.was) return `${group.now} would win over ${group.was} in ${places} (today ${group.was} wins).`;
  if (group.now) return `${group.now} would override another plugin in ${places} (today nothing does).`;
  return `${group.was ?? "A plugin"} would no longer override another plugin in ${places}.`;
}

export function describeExclusion(reasons: readonly LoadOrderExcludeReason[], stillRenamed: boolean): string {
  const parts: string[] = reasons.map((r) =>
    r === "noPluginName"
      ? "Its plugin.txt (the plugin's info file) has no name, so renaming its folder would make the game see a different plugin."
      : "It was put here by hand, and the manager only renames plugins it installed itself. \"Reinstall latest\" on the Installed page lets the manager take it over.",
  );
  if (stillRenamed) parts.push("Its folder still has a number from earlier; use Restore original names.");
  return parts.join(" ");
}

export function describeDependencyNote(note: LoadOrderDependencyNote): string {
  return `${note.plugin} loads before ${note.requires}, which it requires.`;
}
