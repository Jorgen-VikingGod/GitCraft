# GitCraft

An open-source, cross-platform graphical Git client for Linux, macOS and Windows.
GitCraft is a single native program written in Rust, with an [iced](https://iced.rs/) UI. It drives your
installed `git`, so it behaves the same as git on the command line and uses your existing git setup.

| Light | Dark |
| --- | --- |
| ![GitCraft in light mode: commit graph with the commit context menu and its "Reset main to here" submenu open](docs/screenshots/gitcraft-light.png) | ![GitCraft in dark mode: commit graph with the commit context menu and its "Edit commit" submenu open](docs/screenshots/gitcraft-dark.png) |

## Features

- **Commit graph**: virtual-scrolled history of all branches with lane graph, branch/tag/remote badges,
  commit details and per-file diffs (unified or side-by-side, syntax highlighted).
- **Local changes**: stage/unstage/discard files, **hunks or individual lines**; commit, amend,
  commit templates; multi-select with Ctrl/Shift.
- **Branches, tags, remotes**: sidebar with folder trees, ahead/behind counts, checkout (double-click),
  create, rename, delete, upstream tracking, push/delete remote branches and tags, add/edit/remove remotes.
- **Fetch / pull / push** with live progress (merge, rebase or fast-forward pulls; force-with-lease).
- **Merge, rebase (with optional autostash), cherry-pick, revert, reset (soft/mixed/keep/hard), undo last
  commit** from context menus on commits, branch/tag badges and sidebar refs; banner with
  Continue/Skip/Abort while an operation is in progress.
- **Edit history** from the commit menu: edit message, squash/fixup into parent, drop commit.
- **Compare** any two commits (Ctrl/Cmd+click), filter history to the current branch, **reflog** to
  recover lost commits, fast-forward branches without checking them out.
- **Patches** (save commit as patch, apply / `git am`), **.gitignore** from the changes list, stash selected
  files, clean untracked files, checkout a file at a revision, prune remotes, update submodules.
- **Interactive rebase** editor: drag to reorder, pick/reword/edit/squash/fixup/drop, keyboard shortcuts.
- **Conflict resolver**: per-conflict ours/theirs/both choices, editable result, take whole file.
- **Stashes**: create (incl. untracked / staged only), browse, apply, pop, drop.
- **File history**, **blame**, **commit search** (message, author, SHA, content `-S`).
- Repository **tabs**, recent repositories, clone, init.
- Live refresh through file watching, **light/dark** theme, zoom, keyboard shortcuts.

### Pull and rebase options

**Pull** (toolbar, or *Pull…* in the context menu of the current branch) asks for the remote, the remote branch and a strategy:

| Strategy | git command | Result |
| --- | --- | --- |
| Merge | `git pull --no-rebase` | Creates a merge commit if both sides have new commits |
| Rebase | `git pull --rebase` | Replays your local commits on top of the remote branch (linear history) |
| Fast-forward only | `git pull --ff-only` | Only updates if no merge or rebase is needed; otherwise stops |

**Rebase** (context menu of a local or remote branch: *Rebase &lt;current&gt; onto …*) replays the current
branch on top of the chosen one (`git rebase <target>`).

Both dialogs have **Stash and reapply local changes** (`--autostash`): uncommitted changes are stashed
before the operation and restored afterwards, so you can pull or rebase without committing first.
Without it, a rebase refuses to start while tracked files have uncommitted changes (a merge
pull only when they are in the way). If the restored changes conflict,
they stay in the stash list. GitCraft remembers the strategy and the autostash choice for the next
time. Conflicts during a pull or rebase are shown in the operation banner with Continue / Skip / Abort
and can be resolved in the built-in conflict resolver.

### Keyboard shortcuts

| Keys | Action |
| --- | --- |
| Ctrl/⌘ + 1 / 2 | All commits / Local changes |
| Ctrl/⌘ + F | Search commits |
| Ctrl/⌘ + T or O | Open repository |
| Ctrl/⌘ + W | Close tab |
| Ctrl/⌘ + Enter | Commit |
| Ctrl/⌘ + + / − / 0 | Zoom |
| F5 or Ctrl/⌘ + R | Refresh repository |
| ↑ ↓ PgUp PgDn | Move through commits |

In the interactive rebase editor, ↑ ↓ moves the focus, Alt + ↑ ↓ moves the commit, and
<kbd>p</kbd> <kbd>r</kbd> <kbd>e</kbd> <kbd>s</kbd> <kbd>f</kbd> <kbd>d</kbd> set its action.

## Requirements

To run GitCraft you need `git` 2.35 or newer. GitCraft finds it on `PATH`, including the `PATH` of your
login shell when it is started from a desktop launcher.

To build it you also need [rustup](https://rustup.rs/). `rust-toolchain.toml` pins the Rust version,
and rustup installs that version by itself.

## Build & run

```bash
cargo run --release -p gitcraft-ui                  # start GitCraft
cargo run --release -p gitcraft-ui -- path/to/repo  # start GitCraft and open a repository
```

The program takes one optional argument, the repository to open, and is called `gitcraft`
(`target/release/gitcraft`). A second launch hands its repository to the window that is already open
and exits.

Settings, tabs and recent repositories are stored in the user config folder: `~/.config/gitcraft` on
Linux, `~/Library/Application Support/gitcraft` on macOS and `%LOCALAPPDATA%\gitcraft` on Windows. Set
`GITCRAFT_CONFIG_DIR` to use a different folder.

Authentication for remotes uses your normal git setup: credential helpers (e.g. Git Credential Manager,
macOS keychain, libsecret) and the SSH agent. GitCraft never prompts for passwords itself.

## Installers

Installers are built with [cargo-packager](https://github.com/crabnebula-dev/cargo-packager), which
also builds the release binary:

```bash
cargo install cargo-packager --locked --version 0.11.8
cd crates/gitcraft-ui
cargo packager --release --formats deb,appimage     # output goes to target/packages/
```

| Platform | `--formats` | Output |
| --- | --- | --- |
| Linux | `deb,appimage` | `.deb`, `.AppImage` |
| macOS | `dmg` | `.dmg` |
| Windows | `nsis` | NSIS installer `.exe` |

Installers must be built on their own OS; `.github/workflows/rust.yml` builds all of them and attaches
them to the workflow run. The builds are unsigned, apart from the ad-hoc signature that
Apple Silicon requires, so add code-signing for public releases. `git` must be installed on the user's
machine (the `.deb` depends on it).

## Development

```bash
cargo run -p gitcraft-ui -- path/to/repo            # debug build
cargo test --workspace                              # real git against throwaway repositories, no mocks
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

`.github/workflows/rust.yml` runs the tests on Linux, macOS and Windows.

## Architecture

```
gitcraft-ui (iced)  ──function calls──►  gitcraft-core  ──►  git CLI (async subprocess, no shell)
```

| Path | Purpose |
| --- | --- |
| `crates/gitcraft-core/src/runner.rs` | Runs `git` without a shell or terminal; non-interactive editor; per-repo lock |
| `crates/gitcraft-core/src/parsers.rs` | Parsers for log, status v2, refs, diffs, blame, conflict markers; partial patch builder |
| `crates/gitcraft-core/src/graph.rs` | Lane layout for the commit graph |
| `crates/gitcraft-core/src/queries.rs` / `ops.rs` | Read-only queries / mutating operations |
| `crates/gitcraft-core/src/editor.rs` | `GIT_EDITOR` / `GIT_SEQUENCE_EDITOR` mode of the program, used for interactive rebase |
| `crates/gitcraft-core/src/watcher.rs` | File-system change notifications |
| `crates/gitcraft-ui/src/app.rs` | Application state, messages and the update loop |
| `crates/gitcraft-ui/src/command.rs` / `actions.rs` | Git operations as data / dialogs and context menus |
| `crates/gitcraft-ui/src/views/` | Everything drawn on screen: history, changes, diff, sidebar, overlays |

Line staging works by rebuilding a patch with only the selected lines from the parsed diff and applying
it with `git apply --cached [--reverse] --recount`.

### Security

GitCraft talks to git directly and has no web server or API. It opens a single port on `127.0.0.1`, so
that a second launch can pass a repository path to the running window. That port accepts nothing else.
Passwords in remote URLs are masked in what GitCraft shows.

## Author

Jürgen Könnemann

## License

MIT © 2026 Jürgen Könnemann
