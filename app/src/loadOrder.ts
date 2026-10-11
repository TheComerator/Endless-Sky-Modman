// Pure helpers for the optional "Load order" page (delete with the feature; see
// crates/esmm-core/src/load_order.rs for the full list of files).
import type { LoadOrderDependencyNote } from "./bindings/LoadOrderDependencyNote";
import type { LoadOrderExcludeReason } from "./bindings/LoadOrderExcludeReason";
import type { LoadOrderNameDependent } from "./bindings/LoadOrderNameDependent";
import type { LoadOrderNamePlanView } from "./bindings/LoadOrderNamePlanView";
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

export function describeExclusion(
  reasons: readonly LoadOrderExcludeReason[],
  stillRenamed: boolean,
  canName = false,
): string {
  const text: Record<LoadOrderExcludeReason, string> = {
    noPluginName:
      "Its plugin.txt (the plugin's info file) has no name, so renaming its folder would make the game see a different plugin." +
      (canName ? " Give it a name to include it." : ""),
    notLinkedToCatalog:
      "The manager doesn't know this plugin: it was added by hand and isn't linked to a catalog plugin, so it couldn't put the original folder name back. (A hand-added plugin that matches a catalog plugin is linked automatically.)",
    unsupportedFolderName: "Its folder name has characters, or a length, that the manager won't rename.",
  };
  const parts: string[] = reasons.map((r) => text[r]);
  if (stillRenamed) parts.push("Its folder still has a number from earlier; use Restore original names.");
  return parts.join(" ");
}

export function describeDependencyNote(note: LoadOrderDependencyNote): string {
  return `${note.plugin} loads before ${note.requires}, which it requires.`;
}

/** One line per other plugin that mentions the old name, saying what stops working. */
export function describeNameDependent(d: LoadOrderNameDependent): string {
  switch (d.relation) {
    case "requires":
      return `${d.plugin} requires it by its old name, so the manager will report that requirement as missing.`;
    case "optional":
      return `${d.plugin} lists it as optional by its old name, so it won't be recognised there any more.`;
    case "conflicts":
      return `${d.plugin} lists it as a conflict by its old name, so that conflict won't be noticed any more.`;
  }
}

/**
 * What else a new name changes, in plain words. Empty when the name is the folder name, because
 * then the game already calls the plugin that and nothing else needs to change.
 */
export function describeNameConsequences(plan: LoadOrderNamePlanView): string[] {
  if (!plan.identityChanges) return [];
  const lines = [
    `The game will call it "${plan.name}" instead of "${plan.oldIdentity}".`,
    plan.enabled
      ? "Its on/off setting moves to the new name: it is switched on and stays on."
      : "Its on/off setting moves to the new name: it is switched off and stays off.",
  ];
  if (plan.profiles.length > 0) {
    const n = plan.profiles.length;
    const which = n === 1 ? "The profile that includes it switches" : `The ${n} profiles that include it switch`;
    lines.push(`${which} to the new name: ${plan.profiles.join(", ")}.`);
  }
  lines.push("The manager's own record of it is updated too.");
  return lines;
}

/** Where the line goes, and the copy kept beforehand, in plain words. */
export function describeNameWrite(plan: LoadOrderNamePlanView): string {
  return plan.createsFile
    ? `It has no plugin.txt yet, so the manager creates one holding just that line, and keeps a note that there was none at ${plan.backup}.`
    : `The line goes at the top of its plugin.txt; nothing else in the file changes. A copy of the file as it is now is kept at ${plan.backup}.`;
}
