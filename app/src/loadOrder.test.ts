import { describe, expect, it } from "vitest";

import type { LoadOrderWinnerChange } from "./bindings/LoadOrderWinnerChange";
import {
  describeDependencyNote,
  describeExclusion,
  describeGroup,
  describeSuggestion,
  effectiveOrder,
  formatNumber,
  groupWinnerChanges,
  moveItem,
  moveToEnd,
  sameOrder,
  samePlugins,
} from "./loadOrder";

const change = (item: string, was: string | null, now: string | null): LoadOrderWinnerChange => ({
  item,
  was,
  now,
  overridden: was ? [was] : [],
  reason: "",
});

describe("moveItem", () => {
  it("swaps with the neighbour and leaves the original alone", () => {
    const list = ["a", "b", "c"];
    expect(moveItem(list, 1, -1)).toEqual(["b", "a", "c"]);
    expect(moveItem(list, 1, 1)).toEqual(["a", "c", "b"]);
    expect(list).toEqual(["a", "b", "c"]);
  });

  it("does nothing at the ends or for a bad index", () => {
    expect(moveItem(["a", "b"], 0, -1)).toEqual(["a", "b"]);
    expect(moveItem(["a", "b"], 1, 1)).toEqual(["a", "b"]);
    expect(moveItem(["a", "b"], 5, -1)).toEqual(["a", "b"]);
  });
});

describe("moveToEnd", () => {
  it("sends one item to the top or the bottom and keeps the rest in order", () => {
    const list = ["a", "b", "c", "d"];
    expect(moveToEnd(list, 2, "top")).toEqual(["c", "a", "b", "d"]);
    expect(moveToEnd(list, 1, "bottom")).toEqual(["a", "c", "d", "b"]);
    expect(moveToEnd(list, 0, "top")).toEqual(list);
    expect(moveToEnd(list, 3, "bottom")).toEqual(list);
    expect(moveToEnd(list, 9, "top")).toEqual(list);
    expect(list).toEqual(["a", "b", "c", "d"]);
  });
});

describe("sameOrder", () => {
  it("compares position by position", () => {
    expect(sameOrder(["a", "b"], ["a", "b"])).toBe(true);
    expect(sameOrder(["a", "b"], ["b", "a"])).toBe(false);
    expect(sameOrder(["a"], ["a", "b"])).toBe(false);
  });
});

describe("formatNumber", () => {
  it("pads to three digits and marks plugins without a number", () => {
    expect(formatNumber(10)).toBe("010");
    expect(formatNumber(5)).toBe("005");
    expect(formatNumber(990)).toBe("990");
    expect(formatNumber(null)).toBe("none");
  });
});

describe("groupWinnerChanges", () => {
  it("groups by who wins now and who won before, biggest first", () => {
    const groups = groupWinnerChanges([
      change("ship A", "Beta", "Alpha"),
      change("ship B", "Beta", "Alpha"),
      change("ship C", "Beta", "Alpha"),
      change("outfit D", "Alpha", "Gamma"),
    ]);
    expect(groups.map((g) => [g.now, g.was, g.count])).toEqual([
      ["Alpha", "Beta", 3],
      ["Gamma", "Alpha", 1],
    ]);
    expect(groups[0].examples.map((c) => c.item)).toEqual(["ship A", "ship B", "ship C"]);
  });

  it("handles overlaps that appear or disappear", () => {
    const groups = groupWinnerChanges([change("ship A", null, "Alpha"), change("ship B", "Beta", null)]);
    expect(groups).toHaveLength(2);
    expect(describeGroup(groups.find((g) => g.was === null)!)).toContain("today nothing does");
    expect(describeGroup(groups.find((g) => g.now === null)!)).toContain("would no longer override");
  });

  it("is empty for no changes", () => {
    expect(groupWinnerChanges([])).toEqual([]);
  });
});

describe("describeGroup", () => {
  it("says plainly who would win and who wins today", () => {
    expect(describeGroup({ now: "Alpha", was: "Beta", count: 1, examples: [] })).toBe(
      "Alpha would win over Beta in 1 place (today Beta wins).",
    );
    expect(describeGroup({ now: "Alpha", was: "Beta", count: 17, examples: [] })).toContain("in 17 places");
  });
});

describe("describeExclusion", () => {
  it("explains each reason, and a leftover prefix", () => {
    expect(describeExclusion(["noPluginName"], false)).toContain("no name");
    expect(describeExclusion(["notLinkedToCatalog"], false)).toContain("isn't linked to a catalog plugin");
    expect(describeExclusion(["unsupportedFolderName"], false)).toContain("folder name");
    expect(describeExclusion(["noPluginName", "notLinkedToCatalog"], false)).toContain("added by hand");
    expect(describeExclusion(["noPluginName"], true)).toContain("Restore original names");
  });
});

describe("describeDependencyNote", () => {
  it("names the plugin and what it requires", () => {
    expect(describeDependencyNote({ plugin: "A", requires: "B" })).toBe("A loads before B, which it requires.");
  });
});

describe("samePlugins and effectiveOrder", () => {
  it("compares as sets of plugins", () => {
    expect(samePlugins(["a", "b"], ["b", "a"])).toBe(true);
    expect(samePlugins(["a", "b"], ["a", "c"])).toBe(false);
    expect(samePlugins(["a", "a"], ["a", "b"])).toBe(false);
    expect(samePlugins([], [])).toBe(true);
  });

  it("keeps the user's unsaved order only while it fits the installed plugins", () => {
    const saved = ["a", "b", "c"];
    const draft = ["c", "a", "b"];
    expect(effectiveOrder(draft, saved)).toBe(draft);
    expect(effectiveOrder(null, saved)).toBe(saved);
    // A plugin was removed or added meanwhile: the draft no longer fits.
    expect(effectiveOrder(draft, ["a", "b"])).toEqual(["a", "b"]);
    expect(effectiveOrder(draft, ["a", "b", "c", "d"])).toEqual(["a", "b", "c", "d"]);
  });
});

describe("describeSuggestion", () => {
  it("uses the right verb for one or many", () => {
    expect(describeSuggestion(1)).toContain("1 requirement loads first");
    expect(describeSuggestion(3)).toContain("3 requirements load first");
  });
});
