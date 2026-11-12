import { beforeEach, describe, expect, it } from "vitest";

import { pickQuote, QUOTES, resetLastQuote } from "./quotes";

describe("QUOTES", () => {
  it("every quote has text and a credit", () => {
    for (const q of QUOTES) {
      expect(q.text.trim().length).toBeGreaterThan(20);
      expect(q.credit.trim().length).toBeGreaterThan(0);
    }
  });

  it("has no repeated quote", () => {
    const texts = QUOTES.map((q) => q.text);
    expect(new Set(texts).size).toBe(texts.length);
  });

  it("keeps the in-game emotion tags out", () => {
    for (const q of QUOTES) {
      expect(q.text).not.toMatch(/^(Irony|Dismissal|Frustration|Amusement|Humor|Anger)\./);
    }
  });
});

describe("pickQuote", () => {
  beforeEach(resetLastQuote);

  it("maps the random number onto the list, including both ends", () => {
    expect(pickQuote(() => 0)).toBe(QUOTES[0]);
    resetLastQuote();
    expect(pickQuote(() => 0.9999999)).toBe(QUOTES[QUOTES.length - 1]);
  });

  it("never shows the same quote twice in a row", () => {
    const first = pickQuote(() => 0.5);
    const second = pickQuote(() => 0.5);
    expect(second).not.toBe(first);
  });
});
