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

---

## Install records never carried the download's SHA-256, and no test noticed (2026-10-01)

**What went wrong:** `manager::stage_download` called the `Fetcher` but threw away the returned `Downloaded`, then hard-coded `sha256: String::new()`. Every install record written since `manager.rs` landed had an empty hash, although decision A requires it. Found while reading the real `manager.rs` API to wrap it in the Tauri shell, not by any test: the test `Fetcher` even returned a recognizable `"test-sha"`, but no test asserted that it reached the record.

**Why it went wrong:** The tests checked the fields each test was *about* (identity, folder, enabled state) and nothing asserted the field that only mattered for a requirement stated elsewhere (decision A's record contents). A placeholder (`String::new()`) written while wiring things up was never revisited.

**How to prevent it:** When a design decision lists what a record must contain, have at least one end-to-end test assert every listed field, with fixture values that can't be confused with defaults (`"test-sha"`, not `""`). Treat a literal `String::new()`/`Default::default()` in a constructor for persisted data as a review flag. Read the wrapped code itself before building on it, not only its docs.

---

## The Tauri window never appeared under Xvfb, with no error at all (2026-10-01)

**What went wrong:** `target/debug/esmm` under `Xvfb` ran without errors or crashes, but the screen stayed black: only GTK's 10x10 leader window existed, no main window was ever created and no `WebKitWebProcess` started. Sandbox, compositing and DMABUF environment switches changed nothing.

**Why it went wrong:** The shell inherited `DBUS_SESSION_BUS_ADDRESS` pointing at the host's real user session bus (`/run/user/1000/bus`). GTK/WebKitGTK waited on a desktop-portal call over that bus that never got an answer in a session with no desktop, so window creation stalled silently. Thread states (`/proc/<pid>/task/*/wchan`) showed the main loop idling in `poll`, not a crash.

**How to prevent it:** Run headless GTK/WebKit apps inside their own session bus: `DISPLAY=:99 dbus-run-session -- target/debug/esmm`. With that, the window mapped immediately and `xdg-desktop-portal-gtk` started inside the private bus. When a GUI app "runs" headless but shows nothing, list its X windows with `xdotool search --name .` and check whether its child processes exist before trying renderer flags.

---

## First Windows setup: three separate, unrelated-looking failures from `cargo test` (2026-10-01)

**What went wrong:** First-ever build/test on Windows (previously Linux-VPS-only) failed three times in sequence, each looking like a different kind of problem:
1. `cargo test` refused outright: `tauri@2.12.1 requires rustc 1.90`, `sysinfo@0.39.6 requires rustc 1.95`, but the box had rustc 1.89.0.
2. After updating rustc, the `esmm-core` test crate failed to *compile*: `crates/esmm-core/tests/manager.rs` used `std::os::unix::fs::PermissionsExt`/`from_mode` in a new test (`update_all_mid_batch_failure_persists_what_succeeded`, added 2026-10-02) without the `#[cfg(unix)]` gate that every other Unix-permission-trick test already carries.
3. After that compiled, `cargo test`'s separate test-harness binary for the `esmm` (Tauri shell) crate crashed instantly with `STATUS_ENTRYPOINT_NOT_FOUND` (0xc0000139) before any test ran — confirmed via `dumpbin /imports` to be a statically-imported `TaskDialogIndirect` from `comctl32.dll`, which only exists in the "themed" v6 comctl32 that Windows loads only for a process whose *executable* carries an embedded manifest. `tauri_build::build()` (in `build.rs`) embeds that manifest into the real `[[bin]]` target, but not into cargo's separate test-harness binary for the lib crate — so testing `esmm_lib` directly via plain `cargo test` can never work on Windows, regardless of the actual test code.

**Why it went wrong:** The project had only ever been built and tested on a Linux VPS, so nothing about the Windows side (toolchain version floor, Unix-only test tricks, Tauri's Windows manifest requirement) had ever been exercised. Each failure looked unrelated to the others and to "setting up a project," which is exactly why they're worth recording together.

**How to prevent it / what to do each time:**
- Version floor errors from Cargo name the exact required version — `rustup update stable` and retry, don't downgrade dependencies.
- Any test using `std::os::unix::*` needs `#[cfg(unix)]` on the test *and* on any helper function that only that test calls (a helper left ungated throws a `dead_code` warning on Windows once its only caller is gated out — gate the helper too for a clean `cargo clippy`/build).
- `cargo test -p esmm-core` (the pure-Rust crate) is the meaningful test run on any platform; `cargo test -p esmm` (the Tauri shell crate's own unit tests in `app/src-tauri/src/tests.rs`) cannot run via plain `cargo test` on Windows at all — this is a Rust/Tauri Windows limitation, not a project bug. Validate that crate's behavior by actually running the built app (`npm run tauri dev`) instead.
