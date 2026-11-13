import { beforeEach, describe, expect, it } from "vitest";

import { pickQuote, QUOTES, resetLastQuote } from "./quotes";

describe("QUOTES", () => {
  it("every quote has text and a credit", () => {
    for (const q of QUOTES) {
      expect(q.text.trim().length).toBeGreaterThan(20);
      expect(q.credit.trim().length).toBeGreaterThan(0);
    }
  });

  it("keeps every quote short enough for the card", () => {
    for (const q of QUOTES) {
      expect(q.text.length).toBeLessThanOrEqual(220);
    }
  });

  it("has no repeated quote", () => {
    const texts = QUOTES.map((q) => q.text);
    expect(new Set(texts).size).toBe(texts.length);
  });

  it("names no species, faction or region a new player hasn't met yet", () => {
    // Later-game names. Add to this list whenever a new one turns up; the point is that a
    // quote on the start-up card must never introduce them.
    const lateGame =
      /\b(Avgi|Hai|Saryd|Heliarch|Remnant|Arach|Kimek|Quarg|Coalition|Wanderers?|Pug|Korath|Drak|Successors?|Skadenga|Nifel|Archon|Builder|Sheragi|longcows?|Bloodsea|Bloom)\b/i;
    for (const q of QUOTES) {
      expect(`${q.text} ${q.credit}`).not.toMatch(lateGame);
    }
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
