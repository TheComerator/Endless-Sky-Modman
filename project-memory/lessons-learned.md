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
