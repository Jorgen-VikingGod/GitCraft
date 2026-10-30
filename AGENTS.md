# AGENTS.md

Guidance for AI coding agents working on GitCraft, an open-source, cross-platform graphical Git client.
Author: Jürgen Könnemann. License: MIT.

## Architecture

```
Angular 22 SPA (frontend/)  ──HTTP JSON + WebSocket──►  Litestar server (backend/, 127.0.0.1)
                                                          └─► system `git` CLI (asyncio subprocess, no shell)
Electron shell (desktop/) starts a PyInstaller-frozen copy of the server and shows the UI in a window.
```

- The backend never uses libgit2/GitPython. It runs `git` and parses machine-readable output.
- The frontend is built into `backend/gitcraft/static/`, and the server serves it.
- Every `/api` request needs the session token. Send it as header `x-gitcraft-token` or as `?token=`.
  The `repo` query parameter must be a repository opened through `/api/repos/open`.

## Layout

| Path | What lives there |
| --- | --- |
| `backend/gitcraft/git/runner.py` | `run_git()`, `GitError`, env (no prompts, `GIT_EDITOR` helper), per-repo `repo_lock()` |
| `backend/gitcraft/git/parsers.py` | Parsers (log, status v2, refs, diff, blame, conflict markers) and `build_partial_patch()` |
| `backend/gitcraft/git/graph.py` | Commit-graph lane layout |
| `backend/gitcraft/git/queries.py` | Read-only queries |
| `backend/gitcraft/git/ops.py` | Mutating operations (always inside `repo_lock`) |
| `backend/gitcraft/git/editor_helper.py` | Standalone script used as `GIT_EDITOR` / `GIT_SEQUENCE_EDITOR` (must not import gitcraft) |
| `backend/gitcraft/routes/*.py` | Litestar controllers under `/api` |
| `backend/gitcraft/ws.py` | WebSocket: file-watch events and progress lines |
| `backend/gitcraft/app.py` | App factory, token guard, `gitcraft` CLI |
| `backend/packaging/` | PyInstaller spec and frozen entry point |
| `backend/tests/` | pytest suite against throwaway repositories |
| `frontend/src/app/core/` | `Api`, `Events`, `RepoState` (per tab), `Workspace` (tabs + `run()`), `GitActions` (commands, dialogs, context menus), `Ui`, `SettingsStore`, `desktop` bridge |
| `frontend/src/app/<feature>/` | UI: `history`, `changes`, `diff`, `sidebar`, `rebase`, `conflicts`, `inspect`, `dialogs`, `shell`, `shared` |
| `desktop/main.js`, `desktop/preload.js` | Electron main process (server sidecar, window, menu, IPC) and `window.gitcraftDesktop` bridge |
| `desktop/scripts/` | `start.mjs` (dev), `build-server.mjs` (UI + frozen server) |
| `.github/workflows/desktop.yml` | Tests and installers on Linux, macOS and Windows |

## Setup

Requirements: git, [uv](https://docs.astral.sh/uv/) (provides Python 3.10+), and Node 20+
(CI uses Node 22 and Python 3.13).

```bash
cd backend && uv sync            # .venv with the app (editable) + `dev` group; add `--group bundle` for PyInstaller
cd ../frontend && npm ci
cd ../desktop && npm ci          # only for the Electron app
```

- The Python side is a uv project: dependencies in `backend/pyproject.toml`, dev tools in
  `[dependency-groups]` (`dev` is the default, `bundle` = PyInstaller), versions pinned in
  `backend/uv.lock` (commit it). Add packages with `uv add` / `uv add --dev`, never with pip.
- Run Python tools with `uv run …` from `backend/`. Don't use `pip install` into `.venv`.
- uv installs the project **editable**, which matters: a non-editable install copies a stale `static/`
  into site-packages, and the server then serves an old UI. The Electron dev mode runs
  `backend/.venv/bin/python` directly, so `uv sync` must have been run once.

## Run

```bash
scripts/dev.sh                                  # backend :7420 (reload) + ng serve :4200 → http://localhost:4200/?token=dev
cd frontend && npm run build && cd ../backend && uv run gitcraft [repo]   # production-like, opens browser
cd desktop && npm start [repo]                  # Electron dev (needs a built frontend)
```

## Test and build (run before finishing a change)

```bash
cd backend && uv run pytest -q                 # must stay green
cd frontend && npx ng test --watch=false        # Vitest
cd frontend && npx ng build                     # strict TS + strictTemplates; fix warnings too
cd desktop && npm run dist                      # UI + frozen server + installers for the current OS
```

- Backend tests create temporary repos (`tests/conftest.py`: `repo`, `repo_with_history`, `git()`,
  `commit_file()`). Add a test for every new git operation, and run real git rather than mocking it.
- For UI changes, build and click through the affected flow. Headless Chrome and Playwright work for this.

## Conventions

**Backend (Python):**
- Run git through `run_git(cwd, *args, check=..., stdin=..., on_progress=...)`. Never use `shell=True`.
  Put `--` before paths.
- Mutations go in `ops.py`, wrapped in `async with repo_lock(repo):`. Return `_ok(result)`. Operations
  that can stop half-way (merge, rebase, cherry-pick, revert, pull, `am`) use `_may_conflict()`, which
  returns `{ok: False, conflicts, operation}` instead of raising.
- Validate file paths from clients with `_safe_path()`.
- Request bodies subclass `routes.common.Body`, a camelCase msgspec Struct. Query parameters are
  snake_case (`ignore_ws`, `orig_path`). Handlers receive the validated repository as `repo_dir: str`
  (router dependency).
- Errors: raise `GitError` or `ValueError`. Both become HTTP 400 `{error}`.
- Litestar is pinned `<3`. Its 2.x deprecation warnings about inferred parameters are filtered in
  pytest and are expected.
- Long operations accept `opId` and pass `progress(op_id, repo_dir)` from `ws.py`.

**Frontend (Angular 22):**
- Use standalone components with OnPush and signals (`signal`, `computed`, `input()`, `output()`).
  The app is zoneless, so all UI state must live in signals.
- New git commands go into `GitActions`, run through `Workspace.run(label, fn, repo, {success})`, which
  shows a busy state, toasts, progress and a refresh. Dialogs use `Ui.ask()` / `Ui.confirm()`; menus
  use `Ui.openMenu()` with `MenuItem` (one level of `children` submenus is supported).
- Commit context menus are built by `GitActions.commitMenu()`. Branch, tag and remote menus reuse it so
  that commit actions (reset, cherry-pick…) are available everywhere.
- In `effect()`s that start async loads, wrap the call in `untracked(() => …)`. Reading signals
  synchronously inside the load creates refresh loops.
- Theme colours come from CSS variables in `src/styles.scss` (light and dark). Don't hard-code colours.
- Icons: add SVG path data to `shared/icon.ts`.
- Check for desktop-only features with `desktop` from `core/desktop.ts` (null in a browser).

**Desktop (Electron):**
- Context isolation and sandbox stay on; expose new capabilities only through `preload.js` + `ipcMain.handle`.
- The server is started with `--port 0 --exit-on-stdin-close` and a random `GITCRAFT_TOKEN`. Readiness
  is the stdout line `running at <url>`.
- Frozen server: `sys.executable` is the server binary, so git's editor hooks call it with
  `--gitcraft-editor` (handled in `packaging/server_entry.py`). Keep that working when touching
  `runner.editor_command()` or `editor_helper.py`.

## Gotchas

- Hosts like VS Code set `ELECTRON_RUN_AS_NODE=1`. `npm start` clears it; when launching Electron any
  other way, unset it yourself.
- Set `GITCRAFT_USER_DATA=<dir>` when launching the desktop app in tests, so it doesn't hand over to an
  installed GitCraft through the single-instance lock. Set `GITCRAFT_CONFIG_DIR=<dir>` so the
  recent-repos store doesn't touch the user's real config.
- Electron can run headless with `--ozone-platform=headless` (Playwright `_electron`).
- If the requested port is busy, the server silently picks a free one. Read the printed URL.
- Installers can only be built on their own OS; use the GitHub workflow for macOS and Windows.

## Project rules

- GitCraft is an independent, original project. Do not mention, copy or imitate other Git clients
  (names, branding, assets, or wording) anywhere in code, docs or metadata.
- Author and copyright holder: Jürgen Könnemann.
- Still to fill in: `desktop/package.json` `homepage` (placeholder `https://example.com/gitcraft`) and
  the maintainer email (`maintainers@gitcraft.invalid`).
- Never commit build outputs: `backend/gitcraft/static/`, `backend/dist/`, `backend/build/`, `desktop/release/`.
