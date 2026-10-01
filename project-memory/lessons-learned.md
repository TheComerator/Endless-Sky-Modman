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

---

## Passing a generic `fn` item (`fs::rename`) where an injectable `impl Fn(&Path, &Path)` was expected didn't compile (2026-10-01)

**What went wrong:** An earlier session added `install_with(rename: impl Fn(&Path, &Path) -> io::Result<()>, ...)` so tests could inject a failing rename, then called it from `install()` as `install_with(fs::rename, ...)`. This sat uncommitted and didn't actually compile: rustc reported `implementation of Fn is not general enough`, picking one concrete lifetime for `fs::rename`'s generic `P`/`Q` instead of keeping the coercion higher-ranked (`for<'a,'b> Fn(&'a Path, &'b Path)`).

**Why it went wrong:** `fs::rename<P: AsRef<Path>, Q: AsRef<Path>>` is itself generic; coercing a bare generic function *item* straight into a concrete `impl Fn(&Path, &Path)` bound sometimes fails to infer a universally-quantified function pointer, even though the types "look" like they should unify. The diff existed in the working tree (not committed) and nobody had actually run `cargo build` against it before the next session picked up the task.

**How to prevent it:** Wrap a std function with ambiguous-lifetime generics in an explicit closure when passing it to a trait-bound parameter: `|from, to| fs::rename(from, to)` infers correctly because the closure's parameter types come straight from the expected `Fn` bound, not from the function item's own generic signature. More generally: never trust an uncommitted diff is correct just because it exists -- run the build.

---

## Catalog names must actually normalize-match the identity they're meant to resolve, or dependency tests silently test the wrong path (2026-10-01)

**What went wrong:** Early versions of the `manager.rs` integration tests registered catalog entries like `("B-Cat", plugin name "B")` and then had another plugin `require` the identity `"B"`. `resolve::match_catalog` normalizes `"B"` to `"b"` and `"B-Cat"` to `"bcat"` -- they don't match at all, so every "happy path" dependency test (chain, diamond, cycle, enable-a-disabled-dependency, conflicts-via-`requires`) actually exercised the `NoMatch`/`MissingRequirement` path instead of what it claimed to test, and failed once real assertions were added.

**Why it went wrong:** Mentally conflating "the catalog entry for this dependency" with "the dependency," without checking that the normalization rule (lowercase, alphanumeric-only) actually bridges the two strings used in the fixture.

**How to prevent it:** When building a fixture catalog entry that something else's `requires` needs to resolve, make the catalog name either exactly equal to the identity or normalize-identical to it (the real-world case is `Jimmys-Ship-Emporium` / `Jimmy's Ship Emporium`, which *do* normalize the same). Only use a deliberately different catalog name in a test that is specifically exercising `NoMatch`, `Ambiguous`, or `IdentityMismatch`.
