// Plain-language wording for everything the backend reports. Kept free of React so it can
// be unit-tested (format.test.ts): this text is how users understand what a plan will do.
import type { IssueView } from "./bindings/IssueView";
import type { NoteView } from "./bindings/NoteView";
import type { PlanKind } from "./bindings/PlanKind";
import type { PlanView } from "./bindings/PlanView";
import type { StepView } from "./bindings/StepView";
import type { UpdateView } from "./bindings/UpdateView";

export interface Described {
  title: string;
  detail: string;
}

function list(names: string[]): string {
  if (names.length <= 1) return names.join("");
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

export function describeIssue(issue: IssueView): Described {
  switch (issue.kind) {
    case "missingRequirement":
      return {
        title: "Missing requirement",
        detail: `${issue.plugin} requires ${issue.requires}, which isn't installed and enabled.`,
      };
    case "ambiguousRequirement":
      return {
        title: "Ambiguous requirement",
        detail: `${issue.plugin} requires ${issue.requires}, which matches several catalog plugins (${list(issue.candidates)}). Pick the right one to install.`,
      };
    case "identityMismatch":
      return {
        title: "Name mismatch",
        detail: `The catalog plugin ${issue.catalogName} was expected to be ${issue.expected}, but it calls itself ${issue.actual}. The requirement on ${issue.expected} is still unmet.`,
      };
    case "conflict":
      return {
        title: "Conflict",
        detail: `${issue.a} and ${issue.b} declare a conflict and would both be enabled.`,
      };
    case "gameTooOld":
      return {
        title: "Game too old",
        detail: `${issue.plugin} needs Endless Sky ${issue.required} or newer. You have ${issue.actual}.`,
      };
    case "gameVersionUnknown":
      return {
        title: "Game version unknown",
        detail: `${issue.plugin} needs Endless Sky ${issue.required} or newer, but your game's version couldn't be read.`,
      };
    case "requiredBy":
      return {
        title: "Required by other plugins",
        detail: `${list(issue.dependents)} ${issue.dependents.length === 1 ? "requires" : "require"} ${issue.plugin} and would stop working.`,
      };
    case "catalogDownloadFailed":
      return {
        title: "Download failed",
        detail: `Couldn't download ${issue.catalogName}: ${issue.error}`,
      };
  }
}

export function describeNote(note: NoteView): string {
  const state = note.installed ? "installed" : "not installed";
  return `${note.plugin} can optionally use ${note.optional} (${state}).`;
}

export function describeStep(step: StepView): string {
  switch (step.kind) {
    case "install":
      return step.dependency
        ? `Install ${step.identity} ${step.version} (required)`
        : `Install ${step.identity} ${step.version}`;
    case "update":
      return `Update ${step.identity} from ${step.from || "an unknown version"} to ${step.to}`;
    case "enable":
      return `Enable ${step.identity}`;
    case "disable":
      return `Disable ${step.identity}`;
    case "uninstall":
      return `Delete the ${step.folder} folder (${step.identity})`;
  }
}

const VERBS: Record<PlanKind, string> = {
  install: "Install",
  update: "Update",
  enable: "Enable",
  disable: "Disable",
  uninstall: "Uninstall",
  applyProfile: "Switch to profile",
};

export function planTitle(plan: Pick<PlanView, "kind" | "target">): string {
  return `${VERBS[plan.kind]} ${plan.target}`;
}

export function commitLabel(kind: PlanKind, overriding: boolean): string {
  const verb = kind === "applyProfile" ? "Switch" : VERBS[kind];
  return overriding ? `${verb} anyway` : verb;
}

/** Enable/disable plans with nothing to say are committed without asking. */
export function needsReview(plan: PlanView): boolean {
  if (plan.kind !== "enable" && plan.kind !== "disable") return true;
  return plan.issues.length > 0 || plan.notes.some((n) => !n.installed);
}

export function updateLabel(update: UpdateView): string | null {
  switch (update.kind) {
    case "available":
      return `Update: ${update.from} → ${update.to}`;
    case "unknown":
      return "Version unknown";
    case "notInCatalog":
      return "Not in catalog";
    case "unmanaged":
      return "Unmanaged";
    case "upToDate":
    case "unchecked":
      return null;
  }
}

/** Commit-SHA versions (13 catalog entries) are shortened like git does; tags are kept. */
export function shortVersion(version: string): string {
  return /^[0-9a-f]{40}$/i.test(version) ? version.slice(0, 7) : version;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function formatAge(unixSeconds: number, now: number = Date.now()): string {
  const minutes = Math.max(0, Math.round((now / 1000 - unixSeconds) / 60));
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours} h ago`;
  return `${Math.round(hours / 24)} days ago`;
}
