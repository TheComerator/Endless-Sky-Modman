import { describe, expect, it } from "vitest";

import type { IssueView } from "./bindings/IssueView";
import type { PlanView } from "./bindings/PlanView";
import {
  commitLabel,
  describeIssue,
  describeStep,
  formatAge,
  formatBytes,
  shortVersion,
  needsReview,
  planTitle,
  updateLabel,
} from "./format";

function plan(partial: Partial<PlanView>): PlanView {
  return {
    planId: 1,
    kind: "enable",
    target: "X",
    steps: [],
    issues: [],
    notes: [],
    fixes: [],
    resolvableConflicts: [],
    missing: [],
    gameVersion: null,
    unmanagedTarget: false,
    ...partial,
  };
}

describe("describeIssue", () => {
  it("names both sides of a conflict", () => {
    expect(describeIssue({ kind: "conflict", a: "Mega Freight", b: "Mini Freight" }).detail).toBe(
      "Mega Freight and Mini Freight declare a conflict and would both be enabled.",
    );
  });

  it("lists dependents with correct grammar", () => {
    const one = describeIssue({ kind: "requiredBy", plugin: "Lib", dependents: ["A"] });
    const three = describeIssue({ kind: "requiredBy", plugin: "Lib", dependents: ["A", "B", "C"] });
    expect(one.detail).toBe("A requires Lib and would stop working.");
    expect(three.detail).toBe("A, B and C require Lib and would stop working.");
  });

  it("explains game version problems as a minimum", () => {
    expect(
      describeIssue({ kind: "gameTooOld", plugin: "P", required: "0.10.13.1", actual: "0.10.0" })
        .detail,
    ).toBe("P needs Endless Sky 0.10.13.1 or newer. You have 0.10.0.");
    expect(describeIssue({ kind: "gameVersionUnknown", plugin: "P", required: "0.10" }).title).toBe(
      "Game version unknown",
    );
  });

  it("covers every issue kind with a title", () => {
    const kinds: IssueView[] = [
      { kind: "missingRequirement", plugin: "A", requires: "B" },
      { kind: "ambiguousRequirement", plugin: "A", requires: "B", candidates: ["B1", "B2"] },
      { kind: "identityMismatch", expected: "A", catalogName: "A-Cat", actual: "Z" },
      { kind: "catalogDownloadFailed", catalogName: "A", error: "timeout" },
      { kind: "duplicateIdentity", identity: "A", existingFolder: "A-unmanaged", newFolder: "A-Cat" },
    ];
    for (const issue of kinds) {
      expect(describeIssue(issue).title.length).toBeGreaterThan(0);
    }
  });
});

describe("steps and plans", () => {
  it("marks dependencies and unknown update sources", () => {
    expect(
      describeStep({
        kind: "install",
        catalogName: "B",
        identity: "Base",
        folder: "B",
        version: "v1",
        dependency: true,
      }),
    ).toBe("Install Base v1 (required)");
    expect(
      describeStep({ kind: "update", catalogName: "A", identity: "A", folder: "A", from: "", to: "v2" }),
    ).toBe("Update A from an unknown version to v2");
  });

  it("titles and commit labels", () => {
    expect(planTitle({ kind: "applyProfile", target: "Main" })).toBe("Switch to profile Main");
    expect(planTitle({ kind: "updateAll", target: "3 plugins" })).toBe("Update all 3 plugins");
    expect(commitLabel("uninstall", true)).toBe("Uninstall anyway");
    expect(commitLabel("applyProfile", false)).toBe("Switch");
    expect(commitLabel("updateAll", true)).toBe("Update all anyway");
  });

  it("updateAll always needs review, same as every non-enable/disable plan", () => {
    expect(needsReview(plan({ kind: "updateAll" }))).toBe(true);
  });

  it("only skips review for clean enable/disable plans", () => {
    expect(needsReview(plan({ kind: "enable" }))).toBe(false);
    expect(needsReview(plan({ kind: "disable", issues: [{ kind: "conflict", a: "A", b: "B" }] }))).toBe(
      true,
    );
    expect(
      needsReview(
        plan({ kind: "enable", notes: [{ kind: "optionalAvailable", plugin: "A", optional: "B", installed: false }] }),
      ),
    ).toBe(true);
    expect(needsReview(plan({ kind: "install" }))).toBe(true);
  });
});

describe("small formatters", () => {
  it("update labels", () => {
    expect(updateLabel({ kind: "available", from: "v1", to: "v2" })).toBe("Update: v1 → v2");
    expect(updateLabel({ kind: "upToDate" })).toBeNull();
  });

  it("shortens only commit SHAs", () => {
    expect(shortVersion("0123456789abcdef0123456789abcdef01234567")).toBe("0123456");
    expect(shortVersion("v1.0.0-disable.aberrant.blockade")).toBe("v1.0.0-disable.aberrant.blockade");
  });

  it("bytes and ages", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(3 * 1024 * 1024)).toBe("3.0 MB");
    expect(formatAge(1000, 1000 * 1000 + 30 * 1000)).toBe("1 min ago");
    expect(formatAge(0, 3 * 86400 * 1000)).toBe("3 days ago");
  });
});
