"""Mutating git operations. Every public function holds the repository lock."""

from __future__ import annotations

import asyncio
import json
import os
import tempfile
from pathlib import Path
from typing import Any

from . import parsers, queries
from .editor_helper import MESSAGES_FILE
from .runner import GitError, GitResult, ProgressCallback, editor_command, repo_lock, run_git


def _ok(result: GitResult | None = None) -> dict[str, Any]:
    return {"ok": True, "conflicts": False, "message": result.output if result else ""}


async def _may_conflict(repo: str, *args: str, env: dict[str, str] | None = None,
                        on_progress: ProgressCallback | None = None, stdin: str | None = None) -> dict[str, Any]:
    """Run an operation that can stop half-way (conflicts, `edit` steps)."""
    result = await run_git(repo, *args, env=env, check=False, on_progress=on_progress, stdin=stdin)
    if result.code == 0:
        await _cleanup_rebase_messages(repo)
        return _ok(result)
    state = await queries.operation_state(repo)
    if state is None:
        raise GitError(list(args), result.code, result.err, result.out)
    status = await queries.status(repo)
    return {"ok": False, "conflicts": bool(status["conflicts"]), "message": result.output, "operation": state}


async def _cleanup_rebase_messages(repo: str) -> None:
    gd = await queries.git_dir(repo)
    messages = gd / MESSAGES_FILE
    if messages.exists() and await queries.operation_state(repo) is None:
        messages.unlink()


def _safe_path(repo: str, path: str) -> Path:
    root = Path(repo).resolve()
    target = (root / path).resolve()
    if target != root and root not in target.parents:
        raise ValueError(f"Path escapes repository: {path}")
    return target


# ------------------------------------------------------------------ staging


async def stage_files(repo: str, paths: list[str]) -> None:
    async with repo_lock(repo):
        await run_git(repo, "add", "-A", "--", *paths)


async def stage_all(repo: str) -> None:
    async with repo_lock(repo):
        await run_git(repo, "add", "-A")


async def unstage_files(repo: str, paths: list[str]) -> None:
    async with repo_lock(repo):
        if await queries.has_head(repo):
            await run_git(repo, "reset", "-q", "HEAD", "--", *paths)
        else:
            await run_git(repo, "rm", "-q", "--cached", "-r", "--", *paths)


async def unstage_all(repo: str) -> None:
    async with repo_lock(repo):
        if await queries.has_head(repo):
            await run_git(repo, "reset", "-q", "HEAD")
        else:
            await run_git(repo, "rm", "-q", "--cached", "-r", "-f", ".", check=False)


async def discard_files(repo: str, paths: list[str]) -> None:
    async with repo_lock(repo):
        status = await queries.status(repo)
        untracked = {e["path"] for e in status["unstaged"] if e["status"] == "?"}
        tracked = [p for p in paths if p not in untracked]
        for path in paths:
            if path in untracked:
                target = _safe_path(repo, path)
                if target.is_file() or target.is_symlink():
                    target.unlink()
        if tracked:
            await run_git(repo, "checkout", "--", *tracked)


async def apply_selection(
    repo: str, path: str, mode: str, selection: dict[int, list[int] | None], context: int = 3
) -> None:
    """Stage, unstage or discard selected hunks/lines of one file.

    mode: "stage" (index += worktree changes), "unstage" (index -= staged
    changes) or "discard" (worktree -= unstaged changes).
    """
    async with repo_lock(repo):
        staged = mode == "unstage"
        file_diff = await queries.working_diff(repo, path, staged=staged, context=context)
        if file_diff is None or not file_diff["hunks"]:
            raise ValueError("No changes to apply for this file")
        if file_diff["status"] == "?":
            raise ValueError("Untracked files can only be staged as a whole")
        chosen = {int(h): (set(lines) if lines is not None else None) for h, lines in selection.items()}
        if any(h >= len(file_diff["hunks"]) for h in chosen):
            raise ValueError("The diff changed, please refresh")
        reverse = mode != "stage"
        patch = parsers.build_partial_patch(file_diff, chosen, reverse=reverse)
        args = ["apply", "--recount", "--whitespace=nowarn"]
        if mode != "discard":
            args.append("--cached")
        if reverse:
            args.append("-R")
        await run_git(repo, *args, "-", stdin=patch)


# ------------------------------------------------------------------- commit


async def commit(repo: str, message: str, amend: bool = False, sign_off: bool = False) -> dict[str, Any]:
    async with repo_lock(repo):
        args = ["commit", "-F", "-", "--cleanup=strip"]
        if amend:
            args.append("--amend")
        if sign_off:
            args.append("--signoff")
        result = await run_git(repo, *args, stdin=message)
        return _ok(result)


# --------------------------------------------------------------- branches


async def checkout(repo: str, ref: str, kind: str = "local") -> dict[str, Any]:
    async with repo_lock(repo):
        if kind == "remote":
            local_name = ref.split("/", 1)[1] if "/" in ref else ref
            exists = await run_git(repo, "rev-parse", "--verify", "-q", f"refs/heads/{local_name}", check=False)
            if exists.code == 0:
                result = await run_git(repo, "switch", local_name)
            else:
                result = await run_git(repo, "switch", "--track", "-c", local_name, ref)
        elif kind == "commit":
            result = await run_git(repo, "switch", "--detach", ref)
        else:
            result = await run_git(repo, "switch", ref)
        return _ok(result)


async def create_branch(repo: str, name: str, start: str | None, checkout_after: bool) -> dict[str, Any]:
    async with repo_lock(repo):
        if checkout_after:
            result = await run_git(repo, "switch", "-c", name, *([start] if start else []))
        else:
            result = await run_git(repo, "branch", name, *([start] if start else []))
        return _ok(result)


async def rename_branch(repo: str, old: str, new: str) -> dict[str, Any]:
    async with repo_lock(repo):
        return _ok(await run_git(repo, "branch", "-m", old, new))


async def delete_branch(repo: str, name: str, force: bool = False) -> dict[str, Any]:
    async with repo_lock(repo):
        return _ok(await run_git(repo, "branch", "-D" if force else "-d", name))


async def delete_remote_branch(repo: str, remote: str, branch: str, on_progress: ProgressCallback | None) -> dict:
    async with repo_lock(repo):
        return _ok(await run_git(repo, "push", "--progress", remote, "--delete", branch, on_progress=on_progress))


async def set_upstream(repo: str, branch: str, upstream: str | None) -> dict[str, Any]:
    async with repo_lock(repo):
        if upstream:
            return _ok(await run_git(repo, "branch", f"--set-upstream-to={upstream}", branch))
        return _ok(await run_git(repo, "branch", "--unset-upstream", branch))


# -------------------------------------------------------------- tags/remotes


async def create_tag(repo: str, name: str, target: str, message: str | None) -> dict[str, Any]:
    async with repo_lock(repo):
        if message:
            return _ok(await run_git(repo, "tag", "-a", name, target, "-F", "-", stdin=message))
        return _ok(await run_git(repo, "tag", name, target))


async def delete_tag(repo: str, name: str) -> dict[str, Any]:
    async with repo_lock(repo):
        return _ok(await run_git(repo, "tag", "-d", name))


async def push_tag(repo: str, remote: str, name: str, delete: bool, on_progress: ProgressCallback | None) -> dict:
    async with repo_lock(repo):
        args = ["push", "--progress", remote]
        args += ["--delete", f"refs/tags/{name}"] if delete else [f"refs/tags/{name}"]
        return _ok(await run_git(repo, *args, on_progress=on_progress))


async def add_remote(repo: str, name: str, url: str) -> dict[str, Any]:
    async with repo_lock(repo):
        return _ok(await run_git(repo, "remote", "add", name, url))


async def remove_remote(repo: str, name: str) -> dict[str, Any]:
    async with repo_lock(repo):
        return _ok(await run_git(repo, "remote", "remove", name))


async def edit_remote(repo: str, name: str, new_name: str | None, url: str | None) -> dict[str, Any]:
    async with repo_lock(repo):
        if url:
            await run_git(repo, "remote", "set-url", name, url)
        if new_name and new_name != name:
            await run_git(repo, "remote", "rename", name, new_name)
        return _ok()


# ------------------------------------------------------------ remote ops


async def fetch(repo: str, remote: str | None, prune: bool, tags: bool, on_progress: ProgressCallback | None) -> dict:
    async with repo_lock(repo):
        args = ["fetch", "--progress"]
        if prune:
            args.append("--prune")
        if tags:
            args.append("--tags")
        args += [remote] if remote else ["--all"]
        return _ok(await run_git(repo, *args, on_progress=on_progress))


async def pull(repo: str, remote: str | None, branch: str | None, mode: str,
               on_progress: ProgressCallback | None, autostash: bool = False) -> dict[str, Any]:
    """Pull with mode merge, rebase or ff-only; autostash stashes local changes and reapplies them."""
    async with repo_lock(repo):
        args = ["pull", "--progress", "--no-edit"]
        args.append({"rebase": "--rebase", "ff-only": "--ff-only"}.get(mode, "--no-rebase"))
        args.append("--autostash" if autostash else "--no-autostash")
        if remote:
            args.append(remote)
            if branch:
                args.append(branch)
        return await _may_conflict(repo, *args, on_progress=on_progress)


async def push(repo: str, remote: str, branch: str | None, set_upstream_flag: bool, force: bool, tags: bool,
               on_progress: ProgressCallback | None) -> dict[str, Any]:
    async with repo_lock(repo):
        args = ["push", "--progress"]
        if set_upstream_flag:
            args.append("--set-upstream")
        if force:
            args.append("--force-with-lease")
        args.append(remote)
        if branch:
            args.append(f"refs/heads/{branch}:refs/heads/{branch}")
        result = await run_git(repo, *args, on_progress=on_progress)
        if tags:
            await run_git(repo, "push", "--progress", "--tags", remote, on_progress=on_progress)
        return _ok(result)


async def clone(parent: str, url: str, name: str | None, on_progress: ProgressCallback | None) -> str:
    Path(parent).mkdir(parents=True, exist_ok=True)
    target = name or url.rstrip("/").rsplit("/", 1)[-1].rsplit(":", 1)[-1].removesuffix(".git") or "repository"
    destination = Path(parent) / target
    if destination.exists() and any(destination.iterdir()):
        raise ValueError(f"Destination already exists and is not empty: {destination}")
    await run_git(parent, "clone", "--progress", "--recurse-submodules", url, str(destination), on_progress=on_progress)
    return str(destination.resolve())


async def init(path: str) -> str:
    Path(path).mkdir(parents=True, exist_ok=True)
    await run_git(path, "init")
    return await queries.toplevel(path)


# -------------------------------------------------- merge / rebase / reset


async def merge(repo: str, ref: str, mode: str = "default") -> dict[str, Any]:
    async with repo_lock(repo):
        flags = {"no-ff": ["--no-ff"], "ff-only": ["--ff-only"], "squash": ["--squash"]}.get(mode, [])
        return await _may_conflict(repo, "merge", "--no-edit", *flags, ref)


async def rebase(repo: str, onto: str, autostash: bool = False) -> dict[str, Any]:
    async with repo_lock(repo):
        return await _may_conflict(repo, "rebase", "--autostash" if autostash else "--no-autostash", onto)


async def cherry_pick(repo: str, shas: list[str]) -> dict[str, Any]:
    async with repo_lock(repo):
        return await _may_conflict(repo, "cherry-pick", *shas)


async def revert(repo: str, sha: str, mainline: int | None = None) -> dict[str, Any]:
    async with repo_lock(repo):
        args = ["revert", "--no-edit"]
        if mainline:
            args += ["-m", str(mainline)]
        return await _may_conflict(repo, *args, sha)


async def reset(repo: str, target: str, mode: str) -> dict[str, Any]:
    if mode not in ("soft", "mixed", "hard", "keep"):
        raise ValueError(f"Unknown reset mode {mode}")
    async with repo_lock(repo):
        return _ok(await run_git(repo, "reset", f"--{mode}", target))


async def continue_operation(repo: str) -> dict[str, Any]:
    async with repo_lock(repo):
        state = await queries.operation_state(repo)
        if state is None:
            raise ValueError("No operation in progress")
        kind = state["kind"]
        if kind == "merge":
            return await _may_conflict(repo, "commit", "--no-edit")
        return await _may_conflict(repo, kind, "--continue")


async def abort_operation(repo: str) -> dict[str, Any]:
    async with repo_lock(repo):
        state = await queries.operation_state(repo)
        if state is None:
            raise ValueError("No operation in progress")
        result = await run_git(repo, state["kind"], "--abort")
        await _cleanup_rebase_messages(repo)
        return _ok(result)


async def skip_operation(repo: str) -> dict[str, Any]:
    async with repo_lock(repo):
        state = await queries.operation_state(repo)
        if state is None or state["kind"] not in ("rebase", "cherry-pick", "revert", "am"):
            raise ValueError("Nothing to skip")
        return await _may_conflict(repo, state["kind"], "--skip")


# ----------------------------------------------------------- interactive


REBASE_ACTIONS = {"pick", "reword", "edit", "squash", "fixup", "drop"}


async def interactive_rebase(
    repo: str, base: str | None, steps: list[dict[str, Any]], autostash: bool = False
) -> dict[str, Any]:
    """Run `git rebase -i` with a todo list prepared by the UI.

    steps: [{"sha", "action", "message"?}] oldest first. Messages for `reword`
    (and the final `squash` of a chain) are applied by editor_helper.py.
    """
    if not steps:
        raise ValueError("Nothing to rebase")
    lines = []
    messages = {}
    for step in steps:
        action = step["action"]
        if action not in REBASE_ACTIONS:
            raise ValueError(f"Unknown rebase action {action}")
        lines.append(f"{action} {step['sha']}")
        if action in ("reword", "squash") and step.get("message"):
            messages[step["sha"]] = step["message"]
    if steps[0]["action"] in ("squash", "fixup"):
        raise ValueError("The first commit cannot be squashed into a previous commit")

    async with repo_lock(repo):
        gd = await queries.git_dir(repo)
        if messages:
            (gd / MESSAGES_FILE).write_text(json.dumps(messages), encoding="utf-8")
        fd, todo_path = tempfile.mkstemp(prefix="gitcraft-todo-", text=True)
        try:
            with os.fdopen(fd, "w", encoding="utf-8") as handle:
                handle.write("\n".join(lines) + "\n")
            env = {"GIT_SEQUENCE_EDITOR": editor_command("todo", f'"{Path(todo_path).as_posix()}"')}
            args = ["rebase", "-i", "--autostash" if autostash else "--no-autostash"]
            args += [base] if base else ["--root"]
            return await _may_conflict(repo, *args, env=env)
        finally:
            os.unlink(todo_path)


# ----------------------------------------------------------------- stash


async def stash_push(repo: str, message: str | None, include_untracked: bool, staged_only: bool,
                     keep_index: bool, paths: list[str] | None = None) -> dict[str, Any]:
    async with repo_lock(repo):
        args = ["stash", "push"]
        if include_untracked:
            args.append("--include-untracked")
        if staged_only:
            args.append("--staged")
        if keep_index:
            args.append("--keep-index")
        if message:
            args += ["-m", message]
        if paths:
            args += ["--", *paths]
        return _ok(await run_git(repo, *args))


async def stash_apply(repo: str, name: str, pop: bool) -> dict[str, Any]:
    async with repo_lock(repo):
        result = await run_git(repo, "stash", "pop" if pop else "apply", "--index", name, check=False)
        if result.code != 0:
            status = await queries.status(repo)
            if status["conflicts"]:
                return {"ok": False, "conflicts": True, "message": result.output}
            raise GitError(["stash", "apply", name], result.code, result.err, result.out)
        return _ok(result)


async def stash_drop(repo: str, name: str) -> dict[str, Any]:
    async with repo_lock(repo):
        return _ok(await run_git(repo, "stash", "drop", name))


# ------------------------------------------------------------- conflicts


async def conflict_versions(repo: str, path: str) -> dict[str, Any]:
    target = _safe_path(repo, path)
    working = target.read_text(encoding="utf-8", errors="replace") if target.is_file() else None
    base, ours, theirs = await asyncio.gather(*(queries.raw_show(repo, f":{n}:{path}") for n in (1, 2, 3)))
    state = await queries.operation_state(repo)
    rebasing = bool(state and state["kind"] == "rebase")
    return {
        "path": path,
        "base": base,
        "ours": ours,
        "theirs": theirs,
        "working": working,
        "chunks": parsers.parse_conflict_markers(working) if working is not None else [],
        "oursLabel": "Upstream (being rebased onto)" if rebasing else "Ours (HEAD)",
        "theirsLabel": "Your commit (being replayed)" if rebasing else "Theirs (incoming)",
    }


async def resolve_conflict(repo: str, path: str, content: str | None, side: str | None) -> dict[str, Any]:
    async with repo_lock(repo):
        if side in ("ours", "theirs"):
            stage = ":2:" if side == "ours" else ":3:"
            exists = await run_git(repo, "cat-file", "-e", f"{stage}{path}", check=False)
            if exists.code != 0:
                return _ok(await run_git(repo, "rm", "-q", "--", path))
            await run_git(repo, "checkout", f"--{side}", "--", path)
        elif content is not None:
            _safe_path(repo, path).write_text(content, encoding="utf-8", newline="")
        elif not _safe_path(repo, path).exists():
            return _ok(await run_git(repo, "rm", "-q", "--", path))
        return _ok(await run_git(repo, "add", "--", path))


# ----------------------------------------------------------- misc tools


async def checkout_file(repo: str, sha: str, path: str) -> dict[str, Any]:
    """Restore one file (index and working tree) to its state in `sha`."""
    async with repo_lock(repo):
        _safe_path(repo, path)
        return _ok(await run_git(repo, "checkout", sha, "--", path))


async def fast_forward(repo: str, branch: str, target: str | None) -> dict[str, Any]:
    """Fast-forward `branch` to `target` (default: its upstream) without checking it out."""
    async with repo_lock(repo):
        target = target or f"{branch}@{{upstream}}"
        resolved = (await run_git(repo, "rev-parse", "--verify", f"{target}^{{commit}}")).out.strip()
        ancestor = await run_git(repo, "merge-base", "--is-ancestor", f"refs/heads/{branch}", resolved, check=False)
        if ancestor.code != 0:
            raise ValueError(f"{branch} cannot be fast-forwarded to {target} (the branches have diverged)")
        current = (await run_git(repo, "symbolic-ref", "-q", "--short", "HEAD", check=False)).out.strip()
        if current == branch:
            return _ok(await run_git(repo, "merge", "--ff-only", resolved))
        return _ok(await run_git(repo, "update-ref", "-m", f"fast-forward to {target}", f"refs/heads/{branch}", resolved))


async def add_to_gitignore(repo: str, pattern: str) -> dict[str, Any]:
    pattern = pattern.strip()
    if not pattern or "\n" in pattern:
        raise ValueError("Invalid ignore pattern")
    async with repo_lock(repo):
        path = Path(repo) / ".gitignore"
        existing = path.read_text(encoding="utf-8") if path.exists() else ""
        if pattern in existing.splitlines():
            return _ok()
        prefix = "" if not existing or existing.endswith("\n") else "\n"
        with path.open("a", encoding="utf-8", newline="") as handle:
            handle.write(f"{prefix}{pattern}\n")
        # Ignoring a tracked file has no effect unless it is removed from the index.
        target = pattern.lstrip("/")
        tracked = await run_git(repo, "ls-files", "--", target, check=False)
        if tracked.out.strip():
            await run_git(repo, "rm", "-r", "-q", "--cached", "--", target, check=False)
        return _ok()


async def clean(repo: str, directories: bool, ignored: bool, dry_run: bool) -> dict[str, Any]:
    async with repo_lock(repo):
        args = ["clean", "-n" if dry_run else "-f"]
        if directories:
            args.append("-d")
        if ignored:
            args.append("-x")
        result = await run_git(repo, *args)
        files = [line.split(" ", 2)[-1] for line in result.out.splitlines() if line.strip()]
        return {**_ok(result), "files": files}


async def format_patch(repo: str, sha: str) -> str:
    result = await run_git(repo, "format-patch", "-1", "--stdout", sha)
    if not result.out.strip():
        raise ValueError("No patch could be created for this commit (merge commits are not supported)")
    return result.out


async def apply_patch(repo: str, patch: str, mode: str) -> dict[str, Any]:
    """mode: 'am' (create commits from mailbox patches) or 'apply' (only change the working tree/index)."""
    async with repo_lock(repo):
        if mode == "am":
            return await _may_conflict(repo, "am", "--3way", stdin=patch)
        result = await run_git(repo, "apply", "--3way", "--whitespace=nowarn", "-", stdin=patch, check=False)
        if result.code != 0:
            status = await queries.status(repo)
            if status["conflicts"]:
                return {"ok": False, "conflicts": True, "message": result.output}
            raise GitError(["apply"], result.code, result.err, result.out)
        return _ok(result)


async def prune_remote(repo: str, name: str, on_progress: ProgressCallback | None) -> dict[str, Any]:
    async with repo_lock(repo):
        return _ok(await run_git(repo, "remote", "prune", name, on_progress=on_progress))


async def update_submodules(repo: str, on_progress: ProgressCallback | None) -> dict[str, Any]:
    async with repo_lock(repo):
        return _ok(await run_git(repo, "submodule", "update", "--init", "--recursive", "--progress",
                                 on_progress=on_progress))
