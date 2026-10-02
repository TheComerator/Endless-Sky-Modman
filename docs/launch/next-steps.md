# Launch and what follows

A working plan, in order. Items marked **(you)** need a person with accounts or a real machine; the rest can be done in the repo.

## 1. Before the first announcement

- [ ] **(you)** Take 3 to 5 screenshots for the README and posts: the Installed page, the Browse page, an install review dialog, and a conflict dialog. Use real plugins, not test ones. A short GIF of an install-with-dependency is worth more than all of them.
- [ ] **(you)** Install the `.exe` on a PC with no dev tools and run through the first-run steps. Note anything confusing.
- [ ] **(you)** Confirm the repository is public, issues are enabled, and "Discussions" is on if you want a Q&A place.
- [ ] Replace the README's "Install" notes with real file names once the release exists.
- [ ] Create the `v0.1.0` GitHub Release with `docs/release-notes-v0.1.0.md` as its body.
  - Done: the release workflow now runs on version **tags** (`v*`) and makes a draft release with the installers, signed update files and `latest.json`. To release: bump the version in `app/src-tauri/tauri.conf.json` and `app/src-tauri/Cargo.toml`, commit, then `git tag v0.1.0 && git push origin v0.1.0`. When CI finishes, paste the notes into the draft and publish it.
- [ ] **(you)** Add the GitHub secret `TAURI_SIGNING_PRIVATE_KEY` (repo → Settings → Secrets and variables → Actions) with the contents of the private update key, and keep a backup of that key somewhere safe (a password manager). If it's lost, existing installs can never be updated automatically and would need a manual reinstall.
- [ ] **(you)** Read each community's posting rules (Discord channel purpose, subreddit rules on self-promotion) before posting.

## 2. Launch week

- [ ] Post in order of audience size, one at a time, spaced a day or so apart so you can respond properly: Discord, then Reddit, then the forum. Drafts: `announcements.md`.
- [ ] Reply to every comment and issue in the first few days. Early responsiveness is the biggest factor in whether a new tool gets adopted.
- [ ] Keep a short list of what people trip over. Turn repeats into README or in-app wording fixes before adding features.
- [ ] Look for a Linux and a macOS tester in the thread, and ask them specifically to check: installer opens, game detected, plugin installs, launch works.
- [ ] Contact two or three plugin authors whose plugins have dependencies and ask them to check how theirs appear (see `announcements.md`).

## 3. First month

- **Code signing.** Apply to the SignPath Foundation, which offers free signing to open-source projects (an application; it asks for a public repo and builds done in CI). Until then the unsigned warning is documented in the README. A paid certificate (roughly $200 to $400 a year) is the fallback. Neither removes SmartScreen warnings instantly, since reputation builds with downloads.
- **Run the tests in CI.** Right now CI only builds releases. Add a workflow that runs `cargo fmt --check`, `cargo clippy`, `cargo test -p esmm-core`, `cargo test -p esmm` (Linux runs the shell tests Windows can't), and `npm test` on every pull request. This also confirms the hand-edited TypeScript bindings match what the generator produces.
- **Linux and macOS confidence.** Close the open question about Flatpak Steam's save location with a real user, and the "not tested by hand" notes in the README once testers confirm.
- **A support loop.** Label incoming issues (`bug`, `enhancement`, `question`, `needs-info`) and answer within a few days, even if just to say "seen".
- **Version habit.** Decide how versions are bumped (Cargo and `tauri.conf.json` both carry one) and write the steps in CONTRIBUTING.

## 4. Later, when there's demand

- **In-app updates: built.** The app checks GitHub for a newer signed release at startup (switchable in Settings), shows a banner, and installs only on a click and only when no plan is in flight and the game isn't running. This uses a project update key, which is separate from Windows code signing. Still to confirm on a real machine: whether Windows SmartScreen appears for updater-downloaded installers.
- **Share profiles as a file (approved, starts after v0.1.2 is tested):** Export/Import on the Profiles page, serverless; imported plugins are matched by name against the official catalog and missing ones installed with the existing banner.
- **Optional hosted profile sharing** (shareable codes). Shelved on purpose in the v1 design because it needs a server; revisit only if people ask.
- **Manual add of a plugin not in the catalog** (drag a zip or folder), also deliberately out of v1.
- **Plugin author tooling**: a "validate my `plugin.txt`" view showing how the manager reads a plugin's dependencies, which gives authors a reason to adopt it.
- **Translations**, if non-English players show up.

## 5. Decisions that stay open

- **License**: MIT now. Moving to MPL-2.0 or GPL-3.0 is easiest before outside contributors add code, since afterwards each contributor's permission is needed. If the community norm of GPL matters to you, decide soon after launch.
- **Architecture review**: [`project-memory/architecture-review.md`](../../project-memory/architecture-review.md), seven short questions that need your answers.
- **Name and icon**: "Endless Sky Mod Manager" is clear but long, and the project isn't affiliated with the game's developers. Consider a short "unofficial community tool" line in the README and app About box so nobody assumes it's official.
