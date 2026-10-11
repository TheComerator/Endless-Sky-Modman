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

export function sameOrder(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((item, i) => item === b[i]);
}

/** "010" for a folder prefix number, "new" for a plugin that has none yet. */
export function formatNumber(n: number | null): string {
  return n === null ? "new" : String(n).padStart(3, "0");
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
      ? "Its plugin.txt has no name, so renaming its folder would make the game see a different plugin."
      : "It was put here by hand, and the manager only renames plugins it installed itself.",
  );
  if (stillRenamed) parts.push("Its folder still has a number from earlier; use Restore original names.");
  return parts.join(" ");
}

export function describeDependencyNote(note: LoadOrderDependencyNote): string {
  return `${note.plugin} loads before ${note.requires}, which it requires.`;
}
