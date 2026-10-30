"""Deterministic repositories for the parity goldens.

Every commit date and identity is pinned, so object ids are the same on every
run and machine. Each scenario builds one workspace below `root`; the repository
under test is `root/repo`, helpers such as bare remotes live beside it.
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

BASE_TIME = 1_700_000_000
SERVER_TIME = BASE_TIME + 1_000_000
IDENTITY = {
    "GIT_AUTHOR_NAME": "Parity User",
    "GIT_AUTHOR_EMAIL": "parity@example.com",
    "GIT_COMMITTER_NAME": "Parity User",
    "GIT_COMMITTER_EMAIL": "parity@example.com",
}


def isolated_env(home: Path) -> dict[str, str]:
    """Environment shared by the builders and the server: no user or system git config."""
    env = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith("GIT_") and key not in {"LC_ALL", "VIRTUAL_ENV"}
    }
    env.update(IDENTITY)
    env.update(
        {
            "HOME": str(home),
            "USERPROFILE": str(home),
            "XDG_CONFIG_HOME": str(home / ".config"),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": str(home / ".gitconfig"),
        }
    )
    return env


GLOBAL_CONFIG = """\
[init]
\tdefaultBranch = main
[commit]
\tgpgsign = false
[tag]
\tgpgsign = false
[core]
\tautocrlf = false
\tabbrev = 7
[protocol "file"]
\tallow = always
[advice]
\tdetachedHead = false
"""


class Builder:
    def __init__(self, root: Path, env: dict[str, str]) -> None:
        self.root = root
        self.env = env
        self.repo = root / "repo"
        self.clock = BASE_TIME

    def run(self, *args: str, cwd: Path | None = None, input: bytes | None = None, check: bool = True) -> str:
        self.clock += 60
        stamp = f"{self.clock} +0000"
        env = {
            **self.env,
            "GIT_AUTHOR_DATE": stamp,
            "GIT_COMMITTER_DATE": stamp,
            "GIT_EDITOR": "true",
            "GIT_SEQUENCE_EDITOR": "true",
            "GIT_MERGE_AUTOEDIT": "no",
        }
        result = subprocess.run(
            ["git", *args], cwd=cwd or self.repo, env=env, input=input, capture_output=True, check=False
        )
        if check and result.returncode != 0:
            raise RuntimeError(f"git {' '.join(args)} failed: {result.stderr.decode(errors='replace')}")
        return result.stdout.decode(errors="replace")

    def init(self, path: Path | None = None, bare: bool = False) -> Path:
        path = path or self.repo
        path.mkdir(parents=True, exist_ok=True)
        self.run("init", "-q", *(["--bare"] if bare else []), "-b", "main", cwd=path)
        return path

    def write(self, name: str, content: str | bytes, cwd: Path | None = None) -> None:
        path = (cwd or self.repo) / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content.encode() if isinstance(content, str) else content)

    def commit(self, name: str, content: str | bytes, message: str, cwd: Path | None = None) -> str:
        self.write(name, content, cwd)
        self.run("add", "--", name, cwd=cwd)
        self.run("commit", "-q", "-m", message, cwd=cwd)
        return self.run("rev-parse", "HEAD", cwd=cwd).strip()


def numbered(count: int, prefix: str = "line") -> str:
    return "".join(f"{prefix} {i}\n" for i in range(1, count + 1))


def _history(b: Builder) -> None:
    b.init()
    b.commit("a.txt", "one\ntwo\nthree\n", "first")
    b.commit("dir/b.txt", numbered(30), "add b")
    b.commit("a.txt", "one\ntwo\nthree\nfour\n", "second\n\nWith a body line.\n")
    b.commit("dir/b.txt", numbered(30).replace("line 5\n", "line five\n").replace("line 25\n", "line XXV\n"), "edit b")


def empty(b: Builder) -> None:
    b.init()


def unborn_dirty(b: Builder) -> None:
    b.init()
    b.write("new.txt", "fresh\n")
    b.write("staged.txt", "in index\n")
    b.run("add", "--", "staged.txt")


def linear(b: Builder) -> None:
    _history(b)
    b.run("tag", "v1-light", "HEAD~1")
    b.run("tag", "-a", "v2-annotated", "-m", "annotated tag")
    b.run("branch", "feature/one", "HEAD~2")
    b.run("branch", "feature/two", "HEAD~1")
    b.run("config", "commit.template", ".committemplate")
    b.write(".committemplate", "Subject\n\nBody\n")
    b.run("add", "--", ".committemplate")
    b.run("commit", "-q", "-m", "add template")


def merges(b: Builder) -> None:
    b.init()
    b.commit("base.txt", "base\n", "root")
    for branch in ("left", "right", "third"):
        b.run("switch", "-q", "-c", branch, "main")
        b.commit(f"{branch}.txt", f"{branch}\n", f"work on {branch}")
    b.run("switch", "-q", "main")
    b.commit("main.txt", "main\n", "work on main")
    b.run("merge", "-q", "--no-ff", "-m", "merge left", "left")
    b.run("switch", "-q", "-c", "side", "HEAD~1")
    b.commit("side.txt", "side\n", "work on side")
    b.run("switch", "-q", "main")
    b.run("merge", "-q", "--no-ff", "-m", "octopus", "right", "third", "side")
    b.run("switch", "-q", "--orphan", "island")
    b.commit("island.txt", "island\n", "unrelated root")
    b.run("switch", "-q", "main")


def wide(b: Builder) -> None:
    """Many concurrent branches, to exercise lane reuse and the colour cycle."""
    b.init()
    b.commit("root.txt", "root\n", "root")
    for i in range(12):
        b.run("switch", "-q", "-c", f"lane{i:02}", "main")
        b.commit(f"lane{i:02}.txt", "x\n", f"lane {i} first")
        b.commit(f"lane{i:02}.txt", "y\n", f"lane {i} second")
    b.run("switch", "-q", "main")
    for i in range(0, 12, 2):
        b.run("merge", "-q", "--no-ff", "-m", f"merge lane {i}", f"lane{i:02}")


def renames(b: Builder) -> None:
    b.init()
    b.commit("old.txt", numbered(40), "add old")
    b.run("mv", "old.txt", "new.txt")
    b.run("commit", "-q", "-m", "rename old to new")
    b.commit("new.txt", numbered(40).replace("line 1\n", "line one\n"), "edit after rename")
    (b.repo / "moved").mkdir()
    b.run("mv", "new.txt", "moved/new.txt")
    b.write("moved/new.txt", numbered(40).replace("line 1\n", "line one\n").replace("line 2\n", "line two\n"))
    b.run("add", "-A")
    b.run("commit", "-q", "-m", "rename with edit")
    b.run("rm", "-q", "--", "moved/new.txt")
    b.run("commit", "-q", "-m", "delete it")


def dirty(b: Builder) -> None:
    b.init()
    b.commit("multi.txt", numbered(60), "add multi")
    b.commit("gone.txt", "bye\n", "add gone")
    b.commit("old-name.txt", numbered(20, "name"), "add old-name")
    b.commit("both.txt", numbered(10), "add both")
    b.commit("tail.txt", "first\nno newline at end", "add tail")
    b.write("multi.txt", numbered(60).replace("line 5\n", "line 5 edit\nline 5b\n").replace("line 50\n", ""))
    (b.repo / "gone.txt").unlink()
    b.run("mv", "old-name.txt", "new-name.txt")
    b.write("both.txt", numbered(10).replace("line 2\n", "line two\n"))
    b.run("add", "--", "both.txt")
    b.write("both.txt", numbered(10).replace("line 2\n", "line two\n").replace("line 9\n", "line nine\n"))
    b.write("untracked.txt", "new\nfile\n")
    b.write("nested/deep/untracked.txt", "deep\n")
    b.write("no-newline.txt", "no newline")
    b.write("tail.txt", "first\nchanged, still no newline")
    b.write("binary.bin", b"\x00\x01\x02binary\x00")


def _conflicting(b: Builder) -> None:
    b.init()
    b.commit("c.txt", numbered(12), "base")
    b.commit("del.txt", "will be deleted\n", "add del")
    b.run("switch", "-q", "-c", "topic")
    b.write("c.txt", numbered(12).replace("line 3\n", "line 3 topic\n").replace("line 9\n", "line 9 topic\n"))
    b.write("del.txt", "changed on topic\n")
    b.write("add.txt", "topic add\n")
    b.run("add", "-A")
    b.run("commit", "-q", "-m", "topic change")
    b.run("switch", "-q", "main")
    b.write("c.txt", numbered(12).replace("line 3\n", "line 3 main\n").replace("line 9\n", "line 9 main\n"))
    b.run("rm", "-q", "--", "del.txt")
    b.write("add.txt", "main add\n")
    b.run("add", "-A")
    b.run("commit", "-q", "-m", "main change")


def conflict_ready(b: Builder) -> None:
    _conflicting(b)


def merge_conflict(b: Builder) -> None:
    _conflicting(b)
    b.run("merge", "topic", check=False)


def merge_conflict_diff3(b: Builder) -> None:
    _conflicting(b)
    b.run("config", "merge.conflictStyle", "diff3")
    b.run("merge", "topic", check=False)


def rebase_conflict(b: Builder) -> None:
    _conflicting(b)
    b.run("switch", "-q", "topic")
    b.run("rebase", "main", check=False)


def cherry_pick_conflict(b: Builder) -> None:
    _conflicting(b)
    b.run("cherry-pick", "topic", check=False)


def revert_ready(b: Builder) -> None:
    b.init()
    b.commit("r.txt", "one\n", "one")
    b.commit("r.txt", "two\n", "two")
    b.commit("r.txt", "three\n", "three")


def rebase_ready(b: Builder) -> None:
    b.init()
    b.commit("base.txt", "base\n", "base")
    for i in range(1, 6):
        b.commit(f"f{i}.txt", f"{i}\n", f"step {i}")


def remotes(b: Builder) -> None:
    origin = b.init(b.root / "origin.git", bare=True)
    b.init()
    b.run("remote", "add", "origin", f"../{origin.name}")  # relative: merge messages name the URL
    b.run("remote", "add", "tokened", "https://user:s3cret@example.invalid/x.git")
    b.commit("a.txt", numbered(10), "first")
    b.commit("b.txt", "b\n", "second")
    b.run("tag", "shared-tag")
    b.run("push", "-q", "-u", "origin", "main", "--tags")
    b.run("switch", "-q", "-c", "doomed")
    b.commit("doomed.txt", "x\n", "doomed work")
    b.run("push", "-q", "-u", "origin", "doomed")
    b.run("switch", "-q", "main")
    other = b.root / "other"
    b.run("clone", "-q", str(origin), str(other), cwd=b.root)
    b.commit("remote-only.txt", "theirs\n", "remote work", cwd=other)
    b.run("push", "-q", "origin", "HEAD:main", cwd=other)
    b.run("push", "-q", "origin", "--delete", "doomed", cwd=other)
    b.commit("local-only.txt", "ours\n", "local work")


def remote_conflict(b: Builder) -> None:
    origin = b.init(b.root / "origin.git", bare=True)
    b.init()
    b.run("remote", "add", "origin", f"../{origin.name}")
    b.commit("a.txt", numbered(10), "first")
    b.run("push", "-q", "-u", "origin", "main")
    other = b.root / "other"
    b.run("clone", "-q", str(origin), str(other), cwd=b.root)
    b.commit("a.txt", numbered(10).replace("line 2\n", "line two theirs\n"), "theirs", cwd=other)
    b.run("push", "-q", "origin", "HEAD:main", cwd=other)
    b.commit("a.txt", numbered(10).replace("line 2\n", "line two ours\n"), "ours")
    b.run("fetch", "-q", "origin")


def behind(b: Builder) -> None:
    """Local main is strictly behind its upstream, and another branch is checked out."""
    origin = b.init(b.root / "origin.git", bare=True)
    b.init()
    b.run("remote", "add", "origin", f"../{origin.name}")
    b.commit("a.txt", numbered(10), "first")
    b.run("push", "-q", "-u", "origin", "main")
    other = b.root / "other"
    b.run("clone", "-q", str(origin), str(other), cwd=b.root)
    b.commit("theirs.txt", "theirs\n", "theirs", cwd=other)
    b.run("push", "-q", "origin", "HEAD:main", cwd=other)
    b.run("fetch", "-q", "origin")
    b.run("switch", "-q", "-c", "work")


def stashes(b: Builder) -> None:
    b.init()
    b.commit("s.txt", numbered(10), "base")
    b.commit("t.txt", "t\n", "base two")
    b.write("s.txt", numbered(10).replace("line 1\n", "stash one\n"))
    b.run("stash", "push", "-q", "-m", "first stash")
    b.write("t.txt", "t changed\n")
    b.write("extra-untracked.txt", "u\n")
    b.run("stash", "push", "-q", "-u", "-m", "with untracked")
    b.write("s.txt", numbered(10).replace("line 2\n", "local\n"))


def stash_conflict(b: Builder) -> None:
    b.init()
    b.commit("s.txt", "base\n", "base")
    b.write("s.txt", "stashed\n")
    b.run("stash", "push", "-q", "-m", "will conflict")
    b.commit("s.txt", "committed\n", "move on")


def submodules(b: Builder) -> None:
    sub = b.init(b.root / "sub-src")
    b.commit("inner.txt", "inner\n", "inner first", cwd=sub)
    b.init()
    b.commit("top.txt", "top\n", "top")
    b.run("submodule", "add", "-q", "../sub-src", "vendor/sub")
    b.run("commit", "-q", "-m", "add submodule")
    b.commit("inner.txt", "inner 2\n", "inner second", cwd=sub)
    b.run("fetch", "-q", cwd=b.repo / "vendor/sub")
    b.run("checkout", "-q", "origin/main", cwd=b.repo / "vendor/sub")


def submodule_uninit(b: Builder) -> None:
    submodules(b)
    b.run("checkout", "-q", "--", ".")
    b.run("submodule", "--quiet", "deinit", "-f", "--", "vendor/sub")


def worktree(b: Builder) -> None:
    """The repository under test is itself a linked worktree of `main-checkout`."""
    main = b.init(b.root / "main-checkout")
    b.commit("w.txt", "w\n", "first", cwd=main)
    b.run("worktree", "add", "-q", "-b", "wt-branch", str(b.repo), cwd=main)
    b.commit("w2.txt", "w2\n", "in worktree")
    b.write("w.txt", "dirty\n")


def encodings(b: Builder) -> None:
    b.init()
    latin1 = "Grüße aus Köln\nzweite Zeile\ndritte Zeile\nvierte Zeile\n".encode("latin-1")
    b.commit("latin1.txt", latin1, "add latin1")
    b.commit("crlf.txt", b"one\r\ntwo\r\nthree\r\nfour\r\n", "add crlf")
    b.commit("mixed.txt", b"one\ntwo\r\nthree\n", "add mixed endings")
    b.commit("unicode-ÄÖÜ-日本語.txt", "unicode name\nsecond\n", "add unicode name: Grüße 日本語")
    b.commit("ctl.txt", "form\x0cfeed\nnext\x85line\nls sep\n", "control characters in content")
    b.commit("spaces in name.txt", "x\ny\n", "add spaced name")
    b.commit("sub\x0cject.txt".replace("\x0c", "-"), "z\n", "subject with \x0c formfeed and \x1c sep")
    b.write("latin1.txt", "Grüße aus Köln\nZWEITE\ndritte Zeile\nVIERTE\n".encode("latin-1"))
    b.write("crlf.txt", b"one\r\nTWO\r\nthree\r\nFOUR\r\n")
    b.write("unicode-ÄÖÜ-日本語.txt", "unicode name\nchanged\n")
    b.write("new-latin1.txt", "Büro\n".encode("latin-1"))
    b.write("untracked spaced.txt", "z\n")


def odd_names(b: Builder) -> None:
    """Names Windows cannot store; skipped there."""
    b.init()
    b.commit("tab\tname.txt", "x\ny\n", "tab name")
    b.commit('quote"name.txt', "x\ny\n", "quote name")
    b.commit("back\\slash.txt", "x\ny\n", "backslash name")
    b.commit("-leading-dash.txt", "x\ny\n", "dash name")
    b.commit("new\nline.txt", "x\ny\n", "newline name")
    b.commit("script.sh", "#!/bin/sh\n", "add script")
    os.chmod(b.repo / "script.sh", 0o755)
    b.run("add", "--", "script.sh")
    b.run("commit", "-q", "-m", "make executable")
    os.symlink("script.sh", b.repo / "link")
    b.run("add", "--", "link")
    b.run("commit", "-q", "-m", "add symlink")
    b.write("tab\tname.txt", "x\nchanged\n")
    b.write('quote"name.txt', "x\nchanged\n")
    b.write("-leading-dash.txt", "x\nchanged\n")
    b.write("rename source.txt", "r\ns\nt\nu\n")


def big_diff(b: Builder) -> None:
    b.init()
    b.commit("big.txt", numbered(21_000), "add big")
    b.write("big.txt", numbered(21_000, "changed"))


SCENARIOS = {
    fn.__name__: fn
    for fn in (
        empty,
        unborn_dirty,
        linear,
        merges,
        wide,
        renames,
        dirty,
        conflict_ready,
        merge_conflict,
        merge_conflict_diff3,
        rebase_conflict,
        cherry_pick_conflict,
        revert_ready,
        rebase_ready,
        remotes,
        remote_conflict,
        behind,
        stashes,
        stash_conflict,
        submodules,
        submodule_uninit,
        worktree,
        encodings,
        odd_names,
        big_diff,
    )
}
POSIX_ONLY = {"odd_names"}


def available() -> list[str]:
    return [name for name in SCENARIOS if sys.platform != "win32" or name not in POSIX_ONLY]


def build(name: str, root: Path, env: dict[str, str]) -> Path:
    root.mkdir(parents=True)
    builder = Builder(root, env)
    SCENARIOS[name](builder)
    return builder.repo
