# AGENTS.md

Guidance for AI coding agents working on GitCraft, an open-source, cross-platform graphical Git client.
Author: Jürgen Könnemann. License: MIT.

## Architecture

```
gitcraft-ui (egui 0.37) ──function calls──►  gitcraft-core  ──►  system `git` CLI (tokio subprocess, no shell)
```

- GitCraft is one native binary, `gitcraft`. There is no server, web UI or sidecar.
- The core never uses libgit2 or gitoxide. It runs `git` and parses machine-readable output.
- During rebases git starts the `gitcraft` binary itself as its editor (`--gitcraft-editor`).

## Layout

| Path | What lives there |
| --- | --- |
| `crates/gitcraft-core/src/runner.rs` | `run()`, `run_unchecked()`, `git()`, `git_unchecked()`, `GitError`, `Options`, env (no prompts, editor mode), per-repo `repo_lock()`, `MIN_GIT` |
| `crates/gitcraft-core/src/parsers.rs` | Parsers (log, status v2, refs, diff, blame, conflict markers) and the partial patch builder |
| `crates/gitcraft-core/src/graph.rs` | Commit-graph lane layout |
| `crates/gitcraft-core/src/queries.rs` | Read-only queries |
| `crates/gitcraft-core/src/ops.rs` | Mutating operations (always holding `repo_lock`) |
| `crates/gitcraft-core/src/editor.rs` | Non-interactive editor mode, used for `GIT_EDITOR` / `GIT_SEQUENCE_EDITOR` |
| `crates/gitcraft-core/src/model.rs` | Data handed to callers |
| `crates/gitcraft-core/src/watcher.rs` | File-system change notifications for one repository |
| `crates/gitcraft-core/src/store.rs`, `repos.rs`, `paths.rs` | Recent repositories and `config_dir()`; opening repositories; path handling |
| `crates/gitcraft-core/tests/ops.rs` | Operations against real git in throwaway repositories |
| `crates/gitcraft-ui/src/app.rs` | `App`, `Message`, and `App::update`, where all state changes happen |
| `crates/gitcraft-ui/src/gui.rs`, `task.rs` | The eframe window: runs `Task`s on tokio, turns egui input into `Message`s, draws each frame; `Task` is the toolkit-free "work for the shell" type |
| `crates/gitcraft-ui/src/command.rs` | `Command`: every git operation the UI can start, as data (label, success text, how to run it) |
| `crates/gitcraft-ui/src/actions.rs` | `Action`: user-facing commands with their dialogs, and all context menus (`commit_menu_items()`) |
| `crates/gitcraft-ui/src/form.rs`, `menu.rs` | Modal forms; context menus and the menu bar as plain data (`MenuItem`) |
| `crates/gitcraft-ui/src/logic.rs` | UI decisions that need no toolkit, so they can be tested directly |
| `crates/gitcraft-ui/src/theme.rs`, `views/widgets.rs`, `views/icon.rs` | Colours and egui styling; shared widgets; SVG icons |
| `crates/gitcraft-ui/src/views/` | Everything drawn on screen: `shell`, `sidebar`, `history`, `graph_cell`, `details`, `changes`, `diff`, `overlays`, `popups`, `widgets` |
| `crates/gitcraft-ui/src/environment.rs`, `instance.rs`, `settings.rs` | Env for git when not started from a terminal; single-window hand-over; `config.toml` (craft-settings, reads an old `settings.json` once) |
| `crates/gitcraft-ui/src/window.rs`, `appearance.rs` | Who draws the title bar, and the window's size and place in `window.json` (craft-window); the desktop's light or dark setting (craft-platform) |
| `crates/gitcraft-ui/src/i18n.rs`, `i18n/translations.tsv`, `build.rs` | Interface languages (craft-i18n): `tr()` and `trf()`; the table of English, German, French and Spanish; `build.rs` turns it into catalogs |
| `crates/gitcraft-ui/tests/flows.rs` | Drives the real UI against real repositories |
| `crates/gitcraft-core/tests/goldens/` | Recorded commit logs, read by the unit test in `graph.rs` |
| `crates/gitcraft-ui/assets/icon.png` | App icon used for the installers |
| `.github/workflows/rust.yml` | Lint and tests on Linux, macOS and Windows |
| `.github/workflows/release.yml` | Installers (msi, dmg for both Mac chips, deb, AppImage), attached to a published release |
| `.github/workflows/apt-repo.yml`, `packaging/apt/` | Signed apt repository on GitHub Pages, updated after each release |
| `.github/dependabot.yml` | Keeps the pinned actions current |
| `docs/distribution.md` | Installers, the apt repository and its signing key |
| `release-notes/vX.Y.Z.md` | One file per release |

## Setup

Requirements: git 2.35+ and [rustup](https://rustup.rs/). `rust-toolchain.toml` pins the toolchain
(including rustfmt and clippy), and rustup installs it on first use.

## Run

```bash
cargo run -p gitcraft-ui -- [repo]               # debug build
cargo run --release -p gitcraft-ui -- [repo]     # release build
```

## Test and build (run before finishing a change)

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
```

Installers (only on their own OS; `--formats` is `deb,appimage`, `dmg` or `wix`):

```bash
cargo install cargo-packager --locked --version 0.11.8
cd crates/gitcraft-ui && cargo packager --release --formats deb,appimage   # into target/packages/
```

- Core tests use `Fixture` in `crates/gitcraft-core/tests/ops.rs`. Add a test for every new git
  operation, and run real git rather than mocking it.
- UI flows use the `Fixture` in `crates/gitcraft-ui/tests/flows.rs`, which sends `Message`s, runs the
  tasks to the end and checks the result with git itself. Add one for every new user-facing flow.
- Fixtures pin their git config (for example `core.autocrlf=false`), so tests don't depend on the machine.

## Conventions

**Core (`gitcraft-core`):**
- Run git through `runner::git()` / `git_unchecked()`, or `run()` / `run_unchecked()` when you need
  `Options` (`stdin`, `env`, `progress`). Never go through a shell. Put `--` before paths.
- Mutations go in `ops.rs` and hold `repo_lock(repo)` for their whole duration. Operations that can stop
  half-way (merge, rebase, cherry-pick, revert, pull, `am`) use `may_conflict()`, which reports the
  conflict in `OpResult` rather than as an error.
- Validate file paths with `safe_path()`, and refuse values git would read as an option with `check_ref()`.
- Errors are `core::Error`: `Git`, `Invalid` or `Validation`. The last two carry a message for the user.
- Long operations take a `Progress` callback.

**UI (`gitcraft-ui`):**
- All state changes happen in `App::update`. Views only read state and produce `Message`s.
- A new git operation is a `Command` variant with its label, success text and `run()` arm. Start it with
  `Message::Run`, which handles the busy state, toasts, progress and refresh.
- If it needs a dialog, add an `Action` in `actions.rs`. Forms are in `form.rs`; a confirmation is a form
  without fields. Menus are `MenuItem` lists with one level of submenus.
- The menu bar is `App::bar_menus()`, drawn by craft-menu in `views/shell.rs`. Give an entry its key with
  `.key(Shortcut::…)` (the key itself is handled in `App::key`) and a tick or dot with `.marked(Mark::…)`.
- File dialogs never block the window: push `Effect::Ask(Pick::…)`, the shell shows it (craft-dialogs)
  and answers with `Message::Picked(pick, path)`. Add the `Pick` variant and its dialog in `app.rs`.
- Interface text is written in English and goes through `crate::i18n::tr("…")`, or `trf("… {name} …",
  &[("name", value)])` for text with values in it; never build a sentence with `format!`. `MenuItem`,
  `Form`, `Field`, toasts, `Command` labels and the shared widgets (`btn`, `primary`, `placeholder`,
  `check`, field hints…) translate their text themselves. Every new string needs a line in
  `src/i18n/translations.tsv` with all four languages (tab-separated); a test fails for a `tr("…")`
  without one. Plurals are `logic::plural(count, "file")`, whose forms ("1 file", "{count} files") are
  lines of their own.
- Settings are sections of `settings::Settings` (one `[section]` each in `config.toml`). Every struct has
  `#[serde(default)]`, so files from older versions still load.
- Commit menus are built by `commit_menu_items()`. Branch, tag and remote menus reuse it so that commit
  actions (reset, cherry-pick…) are available everywhere.
- Put decisions that need no toolkit in `logic.rs`, with a unit test.
- Colours come from `theme.rs` and widgets from `views/widgets.rs`. Don't hard-code colours in a view.
  The OS only decides light or dark. The look follows the other Craft apps (HybridSliceCraft): neutral
  grey panels, sections as cards (`card_line`, `panel_2`), GitCraft's blue as the accent.
- Icons: add SVG path data to `PATHS` in `views/icon.rs`.
- Don't block the UI thread: slow work runs in a `Task`.
- Views are immediate-mode egui: each frame `views::root` reads `&App` and returns the `Message`s the user's
  input caused. Text fields edit a copy of the text and send the new value as a `Message`. Scroll jumps are
  requested through the `scroll_to` cells, which the view consumes.

**CI (`.github/`):**
- Actions are pinned to a full commit with the version as a comment
  (`actions/checkout@<sha> # v7.0.1`), and every job starts with `step-security/harden-runner` in audit
  mode. Use the same pins as HybridSliceCraft; Dependabot moves them together.
- Release files are attached with `gh release upload`, not a third-party release action.

**All crates:**
- `unsafe_code` is denied in the workspace, and clippy warnings fail CI.
- Add dependencies to the crate's `Cargo.toml` and commit `Cargo.lock`. `eframe`, `egui` and `egui_extras`
  are pinned to exact versions.
- Shared libraries come from `storytold/craft-libs` (`craft-*` crates: atomic, dialogs, i18n, keymap, menu,
  platform, quit, settings, window), all pinned to **one** commit, the same as HybridSliceCraft's. If you
  move one pin, move them all to the same commit.
- Files that belong to the user (settings, the recent list, a file in the working tree) are saved with
  craft-atomic (`store::write_private`, `craft_atomic::atomic_write`), so a failed save leaves the old file.

## Gotchas

- `gitcraft_core::editor::run_if_requested()` must stay the first call in `main`. Keep it working when
  touching `runner::editor_command()` or `editor.rs`. The core's tests use `gitcraft-editor-stub` in its place.
- Set `GITCRAFT_SHOT=<file.png>` to have the app save a picture of its window once the first repository has
  loaded, then quit (handy for looking at the UI without a screenshot tool).
- craft-window decides before the window opens who draws the title bar (`window::decide`); the app draws
  it in `views/frame.rs`. `GITCRAFT_TITLEBAR=app|system` (or `CRAFT_TITLEBAR`) overrides the setting, and
  `GITCRAFT_SYSTEM_FRAME=1` still asks for the system's frame.
- The window's size and place are saved in `window.json` a moment after they stop changing, and on exit.
- `GITCRAFT_SHOT_OPEN=settings` (and `menu`, `pull`, `changes`, `split`, `rebase`, `delete`, `checkout`) opens that
  before the picture is taken. Set `language = "de"` under `[general]` in the test folder's `config.toml`
  to look at a translated window.
- Tests run side by side in one process, and the interface language is global: don't switch it to another
  language in a test (look up with `I18N.t(lang, …)` instead, as `i18n.rs` does).
- Set `GITCRAFT_CONFIG_DIR=<dir>` when launching the app in tests. Otherwise it touches the user's real
  settings and recent repositories, and it may hand over to a running GitCraft through
  `instance.port` and exit immediately.
- Packaged builds rewrite the environment for git in `environment.rs`: AppImage library variables are
  removed, and `PATH` comes from the login shell. A bug that only shows up in an installer may be here.
- `cargo packager` builds the release binary through `before-packaging-command` in
  `crates/gitcraft-ui/Cargo.toml`. A `.deb` without the icon file still builds without a warning, but the
  AppImage fails with `AppImageSquareIcon`.
- Installers can only be built on their own OS; use the GitHub workflow for macOS and Windows.

## Git, branches, releases

- Work on a branch, never on `main`; changes land through pull requests on GitHub
  (`Jorgen-VikingGod/GitCraft`). Branch names: `feature/...`, `fix/...`, `docs/...`, `release/X.Y.Z`.
- Commit subject: what changed, in a plain sentence (`Add git worktree support`, `Publish a signed apt
  repository on GitHub Pages`). The body explains why. Write `Closes #N` when it fixes an issue.
- Don't touch the apt signing setup or the secrets named in `docs/distribution.md` unless asked.

## Making a release

A release is version `X.Y.Z`. The tag is `vX.Y.Z`, and the GitHub release is named `GitCraft X.Y.Z`.

1. **Branch** `release/X.Y.Z` from an up-to-date `main`.
2. **Write the release notes** as `release-notes/vX.Y.Z.md`, in the form of the latest one: an opening
   line `**GitCraft X.Y.Z** ...` summing up the release, then `## Added` / `## Changed` / `## Fixed`
   lists with short bold lead-ins, then `## Downloads`. Collect the content from
   `git log vPREVIOUS..main` and leave out internal changes the user never sees.
3. **Bump the version** in one commit named `Release X.Y.Z`: `version` in `[workspace.package]` of the
   root `Cargo.toml`, `Cargo.lock` (run `cargo build` so it follows), the feature list in `README.md`
   if the release adds a feature, and the new release notes.
4. **Check it:** the three commands under "Test and build" pass.
5. **Push** the branch and open a pull request named `Release X.Y.Z` into `main`.
6. **After the merge, tag the merge commit** on `main` and push the tag:

       git checkout main && git pull
       git tag vX.Y.Z
       git push origin vX.Y.Z

7. **Publish the GitHub release** from that tag:

       gh release create vX.Y.Z --title "GitCraft X.Y.Z" --notes-file release-notes/vX.Y.Z.md

   This starts `release.yml` (installers) and then `apt-repo.yml`. Check that both runs finish and that
   the files are attached to the release.

Pushing, tagging and publishing are visible to everyone and start the release builds. Do steps 5 to 7
only when the maintainer has asked for the release, and confirm before each one.

## Not yet proven

The installers aren't code-signed. Don't claim that a change "works on Windows/macOS" unless it was
checked there: CI runs the tests on all three, which is not the same as using the app.

## Project rules

- GitCraft is an independent, original project. Do not mention, copy or imitate other Git clients
  (names, branding, assets, or wording) anywhere in code, docs or metadata.
- Author and copyright holder: Jürgen Könnemann.
- Never commit build outputs (`target/`).
