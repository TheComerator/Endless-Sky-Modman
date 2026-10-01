# Lessons Learned — Endless Sky Modding

Format: WHAT went wrong, WHY it went wrong, HOW to prevent it.

---

## Assumed the catalog's `version` field was stale (2026-10-01)

**What went wrong:** CLAUDE.md stated the catalog `version` was author-reported at PR time and not live-tracked, so update checking would need per-plugin GitHub API polling. This was written into the design and repeated to Jon as a "reliability caveat."

**Why it went wrong:** Inferred from the manifest's shape (it looks hand-maintained) instead of checking the index repo itself. The repo's `.github/workflows/autoupdate.yml` runs hourly and auto-merges version bumps for 163 of 165 plugins.

**How to prevent it:** Before making a claim about how fresh or reliable a data source is, read the automation that produces it (CI workflows, generator scripts), not just its output.

---

## A regex check reported a false name mismatch (2026-10-01)

**What went wrong:** A quick script comparing catalog names to `plugin.txt` names flagged "Disable Aberrant Blockade" as a mismatch. It actually matched; the `plugin.txt` value was quoted with backticks, which the regex only stripped `"` from.

**Why it went wrong:** The DataNode format has two quoting styles (`"..."` and `` `...` ``), and the ad-hoc check only handled one.

**How to prevent it:** Never parse DataNode files with ad-hoc regex in real code. Use the proper parser (to be written per DataFile.cpp's tokenizer rules), and include backtick-quoted samples in its test fixtures.

---

## Wrote a test that depended on the dev machine's state (2026-10-01)

**What went wrong:** The first version of the real process-list test in `game_state.rs` asserted `detect_game_process() == NotRunning`. That passes on the VPS but would fail for anyone running the test with Endless Sky open. Caught on review before it was committed; changed to assert only that detection isn't `Unknown`.

**Why it went wrong:** Asserted what is true on this box instead of what the code guarantees.

**How to prevent it:** Tests that touch the live system (processes, home dirs, network) should assert only properties that hold on any machine; put exact-outcome checks in tests over injected inputs (here, `game_process_from`).
