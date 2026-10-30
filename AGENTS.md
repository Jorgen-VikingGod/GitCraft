# AGENTS.md

Guidance for AI coding agents working on GitCraft, an open-source, cross-platform graphical Git client.
Author: Jürgen Könnemann. License: MIT.

## Architecture

```
gitcraft-ui (iced 0.14)  ──function calls──►  gitcraft-core  ──►  system `git` CLI (tokio subprocess, no shell)
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
| `crates/gitcraft-ui/src/command.rs` | `Command`: every git operation the UI can start, as data (label, success text, how to run it) |
| `crates/gitcraft-ui/src/actions.rs` | `Action`: user-facing commands with their dialogs, and all context menus (`commit_menu_items()`) |
| `crates/gitcraft-ui/src/form.rs`, `menu.rs` | Modal forms; context menus as plain data (`MenuItem`) |
| `crates/gitcraft-ui/src/logic.rs` | UI decisions that need no toolkit, so they can be tested directly |
| `crates/gitcraft-ui/src/theme.rs`, `views/style.rs`, `views/icon.rs` | Colours and fonts; widget styles; SVG icons |
| `crates/gitcraft-ui/src/views/` | Everything drawn on screen: `shell`, `sidebar`, `history`, `graph_cell`, `details`, `changes`, `diff`, `overlays`, `popups`, `widgets` |
| `crates/gitcraft-ui/src/environment.rs`, `instance.rs`, `settings.rs` | Env for git when not started from a terminal; single-window hand-over; `settings.json` |
| `crates/gitcraft-ui/tests/flows.rs` | Drives the real UI against real repositories |
| `crates/gitcraft-core/tests/goldens/` | Recorded commit logs, read by the unit test in `graph.rs` |
| `crates/gitcraft-ui/assets/icon.png` | App icon used for the installers |
| `.github/workflows/rust.yml` | Lint, tests and installers on Linux, macOS and Windows |

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

Installers (only on their own OS; `--formats` is `deb,appimage`, `dmg` or `nsis`):

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
- Commit menus are built by `commit_menu_items()`. Branch, tag and remote menus reuse it so that commit
  actions (reset, cherry-pick…) are available everywhere.
- Put decisions that need no toolkit in `logic.rs`, with a unit test.
- Colours come from `theme.rs` and widget styles from `views/style.rs`. Don't hard-code either in a view.
  The OS only decides light or dark and the UI font.
- Icons: add SVG path data to `PATHS` in `views/icon.rs`.
- Don't block the UI thread: slow work runs in a `Task`.

**All crates:**
- `unsafe_code` is denied in the workspace, and clippy warnings fail CI.
- Add dependencies to the crate's `Cargo.toml` and commit `Cargo.lock`. `iced` and `native-theme-iced`
  are pinned to exact versions.

## Gotchas

- `gitcraft_core::editor::run_if_requested()` must stay the first call in `main`. Keep it working when
  touching `runner::editor_command()` or `editor.rs`. The core's tests use `gitcraft-editor-stub` in its place.
- Set `GITCRAFT_CONFIG_DIR=<dir>` when launching the app in tests. Otherwise it touches the user's real
  settings and recent repositories, and it may hand over to a running GitCraft through
  `instance.port` and exit immediately.
- Packaged builds rewrite the environment for git in `environment.rs`: AppImage library variables are
  removed, and `PATH` comes from the login shell. A bug that only shows up in an installer may be here.
- `cargo packager` builds the release binary through `before-packaging-command` in
  `crates/gitcraft-ui/Cargo.toml`. A `.deb` without the icon file still builds without a warning, but the
  AppImage fails with `AppImageSquareIcon`.
- Installers can only be built on their own OS; use the GitHub workflow for macOS and Windows.

## Project rules

- GitCraft is an independent, original project. Do not mention, copy or imitate other Git clients
  (names, branding, assets, or wording) anywhere in code, docs or metadata.
- Author and copyright holder: Jürgen Könnemann.
- Never commit build outputs (`target/`).
